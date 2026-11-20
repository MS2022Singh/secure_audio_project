#include <windows.h>
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>
#include <iostream>
#include <string>

static const wchar_t* kFxPropsRoot = L"{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D}";
static const wchar_t* kStreamEffectSlot = L",5";
static const wchar_t* kDisableSysFxName = L"{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5";

int wmain(int argc, wchar_t** argv) {
    // --print-guid: only print the default speakers' endpoint GUID and change nothing
    // (used by install_native_apo.ps1 to take a backup before binding).
    const bool printOnly = (argc > 1 && wcscmp(argv[1], L"--print-guid") == 0);
    if (!printOnly) std::wcout << L"=== Diviora Single-Slot Injector (StreamEffect only) ===" << std::endl;

    HRESULT hr = CoInitialize(NULL);
    if (FAILED(hr)) return 1;

    IMMDeviceEnumerator* pEnum = NULL;
    hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), NULL, CLSCTX_ALL,
                           __uuidof(IMMDeviceEnumerator), (void**)&pEnum);
    if (FAILED(hr) || !pEnum) { CoUninitialize(); return 1; }

    IMMDevice* pDevice = NULL;
    hr = pEnum->GetDefaultAudioEndpoint(eRender, eConsole, &pDevice);
    if (FAILED(hr) || !pDevice) {
        std::wcerr << L"[-] Could not get default render endpoint. Aborting." << std::endl;
        pEnum->Release();
        CoUninitialize();
        return 1;
    }

    LPWSTR pwszId = NULL;
    pDevice->GetId(&pwszId);

    IPropertyStore* pStore = NULL;
    std::wstring friendlyName = L"Default Device";
    if (SUCCEEDED(pDevice->OpenPropertyStore(STGM_READ, &pStore)) && pStore) {
        PROPVARIANT varName;
        PropVariantInit(&varName);
        if (SUCCEEDED(pStore->GetValue(PKEY_Device_FriendlyName, &varName)) && varName.pwszVal) {
            friendlyName = varName.pwszVal;
        }
        PropVariantClear(&varName);
        pStore->Release();
    }

    std::wstring fullId = pwszId ? pwszId : L"";
    size_t lastOpen = fullId.rfind(L"{");
    size_t lastClose = fullId.rfind(L"}");
    std::wstring guidStr;
    if (lastOpen != std::wstring::npos && lastClose != std::wstring::npos && lastClose > lastOpen) {
        guidStr = fullId.substr(lastOpen, lastClose - lastOpen + 1);
    }

    if (printOnly) {
        if (pwszId) CoTaskMemFree(pwszId);
        pDevice->Release();
        pEnum->Release();
        CoUninitialize();
        if (guidStr.empty()) return 1;
        std::wcout << guidStr << std::endl;
        return 0;
    }

    bool success = false;

    if (!guidStr.empty()) {
        std::wstring fxSubKey =
            L"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\MMDevices\\Audio\\Render\\" + guidStr + L"\\FxProperties";

        HKEY hWrite = NULL;
        LONG openResult = RegOpenKeyExW(HKEY_LOCAL_MACHINE, fxSubKey.c_str(), 0, KEY_SET_VALUE, &hWrite);

        if (openResult == ERROR_SUCCESS && hWrite) {
            const wchar_t* clsid = L"{D1B100A1-8888-4444-9999-000000000001}";
            DWORD clsidBytes = (DWORD)((wcslen(clsid) + 1) * sizeof(wchar_t));
            std::wstring streamSlot = std::wstring(kFxPropsRoot) + kStreamEffectSlot;

            LONG r1 = RegSetValueExW(hWrite, streamSlot.c_str(), 0, REG_SZ, (const BYTE*)clsid, clsidBytes);
            DWORD disableSysFx = 0;
            LONG r2 = RegSetValueExW(hWrite, kDisableSysFxName, 0, REG_DWORD,
                                      (const BYTE*)&disableSysFx, sizeof(DWORD));
            RegCloseKey(hWrite);

            if (r1 == ERROR_SUCCESS && r2 == ERROR_SUCCESS) {
                std::wcout << L"[+] Bound (StreamEffect only) to: " << friendlyName << L" [" << guidStr << L"]" << std::endl;
                std::wcout << L"[i] Endpoint GUID (save this): " << guidStr << std::endl;
                success = true;
            } else {
                std::wcerr << L"[-] Value write failed (r1=" << r1 << L" r2=" << r2 << L"). No changes retained." << std::endl;
            }
        } else {
            std::wcerr << L"[-] Cannot open FxProperties (err=" << openResult << L"). No changes made." << std::endl;
        }
    }

    if (pwszId) CoTaskMemFree(pwszId);
    pDevice->Release();
    pEnum->Release();
    CoUninitialize();

    std::wcout << (success ? L"=== Bound successfully. ===" : L"=== Not bound - native audio untouched. ===") << std::endl;
    return success ? 0 : 1;
}