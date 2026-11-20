#include <windows.h>
#include <audioenginebaseapo.h>
#include <iostream>
#include <iomanip>

static const CLSID CLSID_DivioraAPO = { 0xD1B100A1, 0x8888, 0x4444, { 0x99, 0x99, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01 } };
static const IID IID_IAudioSystemEffects2 = { 0xBAFE99D2, 0x7436, 0x44CE, { 0x9E, 0x0E, 0x4D, 0x89, 0xAF, 0xBF, 0xFF, 0x56 } };
static const IID IID_IAudioProcessingObjectPreferredFormats = { 0x82C530C8, 0x0B2F, 0x43DE, { 0x87, 0x58, 0x15, 0x0E, 0x6B, 0x65, 0xED, 0x5C } };

typedef HRESULT (WINAPI* pfnDllGetClassObject)(REFCLSID rclsid, REFIID riid, LPVOID* ppv);

static void PrintResult(const char* testName, HRESULT hr) {
    std::cout << "  " << std::left << std::setw(50) << testName << ": ";
    if (SUCCEEDED(hr)) {
        std::cout << "[SUCCESS] (0x" << std::hex << hr << std::dec << ")" << std::endl;
    } else {
        std::cout << "[FAILED]  (0x" << std::hex << hr << std::dec << ")" << std::endl;
    }
}

static void TestInterfaces(IUnknown* pUnk) {
    if (!pUnk) return;

    IAudioProcessingObject* pAPO = nullptr;
    HRESULT hr = pUnk->QueryInterface(__uuidof(IAudioProcessingObject), (void**)&pAPO);
    PrintResult("QueryInterface (IAudioProcessingObject)", hr);

    IAudioProcessingObjectConfiguration* pAPOConfig = nullptr;
    hr = pUnk->QueryInterface(__uuidof(IAudioProcessingObjectConfiguration), (void**)&pAPOConfig);
    PrintResult("QueryInterface (IAudioProcessingObjectConfig)", hr);

    IAudioProcessingObjectRT* pAPORT = nullptr;
    hr = pUnk->QueryInterface(__uuidof(IAudioProcessingObjectRT), (void**)&pAPORT);
    PrintResult("QueryInterface (IAudioProcessingObjectRT)", hr);

    IAudioSystemEffects* pSysFx = nullptr;
    hr = pUnk->QueryInterface(__uuidof(IAudioSystemEffects), (void**)&pSysFx);
    PrintResult("QueryInterface (IAudioSystemEffects)", hr);

    IUnknown* pSysFx2 = nullptr;
    hr = pUnk->QueryInterface(IID_IAudioSystemEffects2, (void**)&pSysFx2);
    PrintResult("QueryInterface (IAudioSystemEffects2)", hr);

    IUnknown* pPrefFormats = nullptr;
    hr = pUnk->QueryInterface(IID_IAudioProcessingObjectPreferredFormats, (void**)&pPrefFormats);
    PrintResult("QueryInterface (IAudioProcessingObjectPrefFormats)", hr);

    if (pAPO) {
        APO_REG_PROPERTIES* pProps = nullptr;
        hr = pAPO->GetRegistrationProperties(&pProps);
        PrintResult("IAudioProcessingObject::GetRegistrationProperties", hr);
        if (SUCCEEDED(hr) && pProps) {
            std::wcout << L"    -> Friendly Name: " << pProps->szFriendlyName << std::endl;
            std::wcout << L"    -> Flags: 0x" << std::hex << pProps->Flags << std::dec << std::endl;
            CoTaskMemFree(pProps);
        }

        hr = pAPO->Initialize(0, nullptr);
        PrintResult("IAudioProcessingObject::Initialize", hr);
    }

    if (pPrefFormats) pPrefFormats->Release();
    if (pSysFx2) pSysFx2->Release();
    if (pSysFx) pSysFx->Release();
    if (pAPORT) pAPORT->Release();
    if (pAPOConfig) pAPOConfig->Release();
    if (pAPO) pAPO->Release();
}

int main() {
    std::cout << "=== Diviora APO Direct & COM Instantiation Harness ===" << std::endl;

    HRESULT hrCo = CoInitializeEx(NULL, COINIT_MULTITHREADED);

    // 1. Direct DLL Class Factory Test
    std::cout << "\n[1] Testing Direct DllGetClassObject via LoadLibrary..." << std::endl;
    HMODULE hDll = LoadLibraryW(L"C:\\Program Files\\Diviora\\DivioraAPO.dll");
    if (!hDll) {
        std::cerr << "[-] LoadLibrary failed on C:\\Program Files\\Diviora\\DivioraAPO.dll (Err=" << GetLastError() << ")" << std::endl;
    } else {
        pfnDllGetClassObject getClassObject = (pfnDllGetClassObject)GetProcAddress(hDll, "DllGetClassObject");
        if (!getClassObject) {
            std::cerr << "[-] GetProcAddress failed for DllGetClassObject." << std::endl;
        } else {
            IClassFactory* pFactory = nullptr;
            HRESULT hr = getClassObject(CLSID_DivioraAPO, IID_IClassFactory, (void**)&pFactory);
            PrintResult("DllGetClassObject (IClassFactory)", hr);

            if (SUCCEEDED(hr) && pFactory) {
                IUnknown* pInstance = nullptr;
                hr = pFactory->CreateInstance(NULL, IID_IUnknown, (void**)&pInstance);
                PrintResult("IClassFactory::CreateInstance (IID_IUnknown)", hr);

                if (SUCCEEDED(hr) && pInstance) {
                    TestInterfaces(pInstance);
                    pInstance->Release();
                }
                pFactory->Release();
            }
        }
        FreeLibrary(hDll);
    }

    // 2. System COM CoCreateInstance Test
    std::cout << "\n[2] Testing System CoCreateInstance via Registry..." << std::endl;
    IUnknown* pUnk = nullptr;
    HRESULT hrCom = CoCreateInstance(CLSID_DivioraAPO, NULL, CLSCTX_INPROC_SERVER, IID_IUnknown, (void**)&pUnk);
    PrintResult("CoCreateInstance (IID_IUnknown)", hrCom);

    if (SUCCEEDED(hrCom) && pUnk) {
        TestInterfaces(pUnk);
        pUnk->Release();
    }

    if (SUCCEEDED(hrCo)) CoUninitialize();
    std::cout << "\n=== Diagnostics Run Finished ===" << std::endl;
    return 0;
}