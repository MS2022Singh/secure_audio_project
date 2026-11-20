#include <windows.h>
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>
#include <iostream>
#include <string>

int main() {
    std::wcout << L"=== Diviora Multi-Endpoint System-Wide Injector ===" << std::endl;

    HRESULT hr = CoInitialize(NULL);
    if (FAILED(hr)) return 1;

    IMMDeviceEnumerator* pEnum = NULL;
    hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), NULL, CLSCTX_ALL, __uuidof(IMMDeviceEnumerator), (void**)&pEnum);
    if (FAILED(hr) || !pEnum) {
        CoUninitialize();
        return 1;
    }

    IMMDeviceCollection* pCollection = NULL;
    hr = pEnum->EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE, &pCollection);
    if (SUCCEEDED(hr) && pCollection) {
        UINT count = 0;
        pCollection->GetCount(&count);
        const wchar_t* clsid = L"{D1B100A1-8888-4444-9999-000000000001}";
        DWORD clsidBytes = (DWORD)((wcslen(clsid) + 1) * sizeof(wchar_t));
        DWORD disableSysFx = 0;

        for (UINT i = 0; i < count; ++i) {
            IMMDevice* pDevice = NULL;
            if (SUCCEEDED(pCollection->Item(i, &pDevice)) && pDevice) {
                LPWSTR pwszId = NULL;
                pDevice->GetId(&pwszId);

                IPropertyStore* pStore = NULL;
                std::wstring friendlyName = L"Audio Device";
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
                std::wstring guidStr = L"";
                if (lastOpen != std::wstring::npos && lastClose != std::wstring::npos && lastClose > lastOpen) {
                    guidStr = fullId.substr(lastOpen, lastClose - lastOpen + 1);
                }

                if (!guidStr.empty()) {
                    std::wstring fxSubKey = L"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\MMDevices\\Audio\\Render\\" + guidStr + L"\\FxProperties";
                    HKEY hWrite = NULL;
                    DWORD disp = 0;
                    if (RegCreateKeyExW(HKEY_LOCAL_MACHINE, fxSubKey.c_str(), 0, NULL, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE | KEY_QUERY_VALUE, NULL, &hWrite, &disp) == ERROR_SUCCESS) {
                        RegSetValueExW(hWrite, L"{d04e05a5-594b-4fb6-a80d-01af5eed7d1d},0", 0, REG_SZ, (const BYTE*)clsid, clsidBytes);
                        RegSetValueExW(hWrite, L"{d04e05a5-594b-4fb6-a80d-01af5eed7d1d},5", 0, REG_SZ, (const BYTE*)clsid, clsidBytes);
                        RegSetValueExW(hWrite, L"{d04e05a5-594b-4fb6-a80d-01af5eed7d1d},6", 0, REG_SZ, (const BYTE*)clsid, clsidBytes);
                        RegSetValueExW(hWrite, L"{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},1", 0, REG_SZ, (const BYTE*)clsid, clsidBytes);
                        RegSetValueExW(hWrite, L"{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},2", 0, REG_SZ, (const BYTE*)clsid, clsidBytes);
                        RegSetValueExW(hWrite, L"{b3f8fa53-0004-438e-9003-51a46e139bfc},2", 0, REG_DWORD, (const BYTE*)&disableSysFx, sizeof(DWORD));
                        RegCloseKey(hWrite);
                        std::wcout << L"  [+] Bound & Enabled on: " << friendlyName << L" [" << guidStr << L"]" << std::endl;
                    }
                }

                if (pwszId) CoTaskMemFree(pwszId);
                pDevice->Release();
            }
        }
        pCollection->Release();
    }

    pEnum->Release();
    CoUninitialize();
    return 0;
}