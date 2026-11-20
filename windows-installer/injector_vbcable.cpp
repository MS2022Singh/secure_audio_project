#include <windows.h>
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>
#include <iostream>
#include <string>

static const wchar_t* kFxPropsRoot = L"{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D}";
static const wchar_t* kStreamEffectSlot = L",5";
static const wchar_t* kDisableSysFxName = L"{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5";

int main(int argc, char** argv) {
    std::wcout << L"=== Diviora VB-Cable Injector ===" << std::endl;

    if (argc < 2) {
        std::wcerr << L"[-] Usage: injector_vbcable.exe \"<partial friendly name to match, e.g. CABLE Input>\"" << std::endl;
        return 1;
    }

    std::string narrowMatch(argv[1]);
    std::wstring matchName(narrowMatch.begin(), narrowMatch.end());

    HRESULT hr = CoInitialize(NULL);
    if (FAILED(hr)) return 1;

    IMMDeviceEnumerator* pEnum = NULL;
    hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), NULL, CLSCTX_ALL,
                           __uuidof(IMMDeviceEnumerator), (void**)&pEnum);
    if (FAILED(hr) || !pEnum) { CoUninitialize(); return 1; }

    IMMDeviceCollection* pCollection = NULL;
    hr = pEnum->EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE, &pCollection);
    if (FAILED(hr) || !pCollection) {
        std::wcerr << L"[-] Could not enumerate render endpoints." << std::endl;
        pEnum->Release();
        CoUninitialize();
        return 1;
    }

    UINT count = 0;
    pCollection->GetCount(&count);
    bool success = false;

    for (UINT i = 0; i < count; ++i) {
        IMMDevice* pDevice = NULL;
        if (FAILED(pCollection->Item(i, &pDevice)) || !pDevice) continue;

        IPropertyStore* pStore = NULL;
        std::wstring friendlyName;
        if (SUCCEEDED(pDevice->OpenPropertyStore(STGM_READ, &pStore)) && pStore) {
            PROPVARIANT varName;
            PropVariantInit(&varName);
            if (SUCCEEDED(pStore->GetValue(PKEY_Device_FriendlyName, &varName)) && varName.pwszVal) {
                friendlyName = varName.pwszVal;
            }
            PropVariantClear(&varName);
            pStore->Release();
        }

        if (friendlyName.find(matchName) == std::wstring::npos) {
            pDevice->Release();
            continue;
        }

        LPWSTR pwszId = NULL;
        pDevice->GetId(&pwszId);
        std::wstring fullId = pwszId ? pwszId : L"";
        size_t lastOpen = fullId.rfind(L"{");
        size_t lastClose = fullId.rfind(L"}");
        std::wstring guidStr;
        if (lastOpen != std::wstring::npos && lastClose != std::wstring::npos && lastClose > lastOpen) {
            guidStr = fullId.substr(lastOpen, lastClose - lastOpen + 1);
        }

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
                    std::wcout << L"[+] Bound to: " << friendlyName << L" [" << guidStr << L"]" << std::endl;
                    std::wcout << L"[i] Endpoint GUID (save this): " << guidStr << std::endl;
                    success = true;
                } else {
                    std::wcerr << L"[-] Value write failed (r1=" << r1 << L" r2=" << r2 << L")" << std::endl;
                }
            } else {
                std::wcerr << L"[-] Cannot open FxProperties (err=" << openResult << L")" << std::endl;
            }
        }

        if (pwszId) CoTaskMemFree(pwszId);
        pDevice->Release();
        break;
    }

    if (!success) {
        std::wcerr << L"[-] No matching active endpoint found for \"" << matchName << L"\". No changes made." << std::endl;
    }

    pCollection->Release();
    pEnum->Release();
    CoUninitialize();
    return success ? 0 : 1;
}