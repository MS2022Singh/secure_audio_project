#include "DivioraAPO.h"
#include <unknwn.h>
#include <windows.h>

class DivioraClassFactory : public IClassFactory {
private:
    LONG m_cRef;
public:
    DivioraClassFactory() : m_cRef(1) {}
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObject) override {
        if (!ppvObject) return E_POINTER;
        if (riid == IID_IUnknown || riid == IID_IClassFactory) {
            *ppvObject = static_cast<IClassFactory*>(this);
            AddRef();
            return S_OK;
        }
        *ppvObject = nullptr;
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override {
        ULONG ulRef = InterlockedDecrement(&m_cRef);
        if (ulRef == 0) delete this;
        return ulRef;
    }
    STDMETHODIMP CreateInstance(IUnknown* pUnkOuter, REFIID riid, void** ppvObject) override {
        if (!ppvObject) return E_POINTER;
        *ppvObject = nullptr;
        if (pUnkOuter != nullptr) return CLASS_E_NOAGGREGATION;
        DivioraEndpointAPO* pApo = new (std::nothrow) DivioraEndpointAPO();
        if (!pApo) return E_OUTOFMEMORY;
        HRESULT hr = pApo->QueryInterface(riid, ppvObject);
        pApo->Release();
        return hr;
    }
    STDMETHODIMP LockServer(BOOL fLock) override { return S_OK; }
};

STDAPI DllGetClassObject(REFCLSID rclsid, REFIID riid, void** ppv) {
    if (!ppv) return E_POINTER;
    *ppv = nullptr;
    if (InlineIsEqualGUID(rclsid, CLSID_DivioraEndpointAPO)) {
        DivioraClassFactory* pFactory = new (std::nothrow) DivioraClassFactory();
        if (!pFactory) return E_OUTOFMEMORY;
        HRESULT hr = pFactory->QueryInterface(riid, ppv);
        pFactory->Release();
        return hr;
    }
    return CLASS_E_CLASSNOTAVAILABLE;
}

STDAPI DllCanUnloadNow() { return S_FALSE; }

STDAPI DllRegisterServer() {
    HKEY hKey = nullptr;
    WCHAR szModule[MAX_PATH];
    HMODULE hModule = nullptr;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, (LPCWSTR)&DllRegisterServer, &hModule);
    GetModuleFileNameW(hModule, szModule, MAX_PATH);
    if (RegCreateKeyW(HKEY_CLASSES_ROOT, L"CLSID\\{D1B100A1-8888-4444-9999-000000000001}", &hKey) != ERROR_SUCCESS) return E_FAIL;
    RegSetValueW(hKey, nullptr, REG_SZ, L"Diviora Endpoint APO", 0);
    RegCloseKey(hKey);
    if (RegCreateKeyW(HKEY_CLASSES_ROOT, L"CLSID\\{D1B100A1-8888-4444-9999-000000000001}\\InprocServer32", &hKey) != ERROR_SUCCESS) return E_FAIL;
    RegSetValueW(hKey, nullptr, REG_SZ, szModule, (DWORD)(wcslen(szModule) + 1) * sizeof(WCHAR));
    RegSetValueExW(hKey, L"ThreadingModel", 0, REG_SZ, (const BYTE*)L"Both", 10);
    RegCloseKey(hKey);
    return S_OK;
}

STDAPI DllUnregisterServer() {
    HKEY hKey = nullptr;
    if (RegOpenKeyW(HKEY_CLASSES_ROOT, L"CLSID", &hKey) == ERROR_SUCCESS) {
        RegDeleteTreeW(hKey, L"{D1B100A1-8888-4444-9999-000000000001}");
        RegCloseKey(hKey);
    }
    return S_OK;
}
