#include <windows.h>
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>
#include <iostream>
#include <string>
#include <vector>

static const wchar_t* kModesRoot = L"{d3993a3f-99c2-4402-b5ec-a92a0367664b}";
static const wchar_t* kFxPropsRoot = L"{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D}";
static const wchar_t* kDivioraCLSID = L"{D1B100A1-8888-4444-9999-000000000001}";

static void PrependMultiSz(HKEY hKey, const std::wstring& slotName) {
    BYTE buf[4096] = {0};
    DWORD bufBytes = sizeof(buf);
    DWORD type = 0;
    LONG r = RegQueryValueExW(hKey, slotName.c_str(), NULL, &type, buf, &bufBytes);

    std::vector<wchar_t> newMultiSz;
    // Push Diviora CLSID + null terminator
    const size_t clsidLen = wcslen(kDivioraCLSID);
    newMultiSz.insert(newMultiSz.end(), kDivioraCLSID, kDivioraCLSID + clsidLen);
    newMultiSz.push_back(L'\0');

    if (r == ERROR_SUCCESS) {
        wchar_t* cur = (wchar_t*)buf;
        while (*cur) {
            if (_wcsicmp(cur, kDivioraCLSID) != 0) {
                size_t len = wcslen(cur);
                newMultiSz.insert(newMultiSz.end(), cur, cur + len);
                newMultiSz.push_back(L'\0');
            }
            cur += wcslen(cur) + 1;
        }
    }

    // Double null-terminate for REG_MULTI_SZ
    newMultiSz.push_back(L'\0');

    DWORD totalBytes = (DWORD)(newMultiSz.size() * sizeof(wchar_t));
    RegSetValueExW(hKey, slotName.c_str(), 0, REG_MULTI_SZ, (const BYTE*)newMultiSz.data(), totalBytes);

    // Also set standard REG_SZ for legacy compatibility
    RegSetValueExW(hKey, (std::wstring(kFxPropsRoot) + slotName.substr(slotName.find(L','))).c_str(), 0, REG_SZ, (const BYTE*)kDivioraCLSID, (DWORD)((clsidLen + 1) * sizeof(wchar_t)));
    std::wcout << L"  [+] Multi-string prepended into: " << slotName << std::endl;
}

int main() {
    std::wcout << L"=== Diviora Multi-SZ Chain Injector ===" << std::endl;

    HRESULT hr = CoInitialize(NULL);
    if (FAILED(hr)) return 1;

    IMMDeviceEnumerator* pEnum = NULL;
    hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), NULL, CLSCTX_ALL, __uuidof(IMMDeviceEnumerator), (void**)&pEnum);
    if (FAILED(hr) || !pEnum) { CoUninitialize(); return 1; }

    IMMDevice* pDevice = NULL;
    hr = pEnum->GetDefaultAudioEndpoint(eRender, eConsole, &pDevice);
    if (FAILED(hr) || !pDevice) { pEnum->Release(); CoUninitialize(); return 1; }

    LPWSTR pwszId = NULL;
    pDevice->GetId(&pwszId);

    std::wstring fullId = pwszId ? pwszId : L"";
    size_t lastOpen = fullId.rfind(L"{");
    size_t lastClose = fullId.rfind(L"}");
    std::wstring guidStr;
    if (lastOpen != std::wstring::npos && lastClose != std::wstring::npos && lastClose > lastOpen) {
        guidStr = fullId.substr(lastOpen, lastClose - lastOpen + 1);
    }

    bool success = false;
    if (!guidStr.empty()) {
        std::wstring fxSubKey = L"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\MMDevices\\Audio\\Render\\" + guidStr + L"\\FxProperties";
        HKEY hWrite = NULL;
        LONG openResult = RegOpenKeyExW(HKEY_LOCAL_MACHINE, fxSubKey.c_str(), 0, KEY_SET_VALUE | KEY_QUERY_VALUE, &hWrite);

        if (openResult == ERROR_SUCCESS && hWrite) {
            PrependMultiSz(hWrite, std::wstring(kModesRoot) + L",5");
            PrependMultiSz(hWrite, std::wstring(kModesRoot) + L",6");
            PrependMultiSz(hWrite, std::wstring(kModesRoot) + L",7");
            PrependMultiSz(hWrite, std::wstring(kModesRoot) + L",11");
            PrependMultiSz(hWrite, std::wstring(kModesRoot) + L",12");

            DWORD disableSysFx = 0;
            RegSetValueExW(hWrite, L"{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5", 0, REG_DWORD, (const BYTE*)&disableSysFx, sizeof(DWORD));
            RegCloseKey(hWrite);
            success = true;
        }
    }

    if (pwszId) CoTaskMemFree(pwszId);
    pDevice->Release();
    pEnum->Release();
    CoUninitialize();

    return success ? 0 : 1;
}