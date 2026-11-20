#ifndef UNICODE
#define UNICODE
#endif
#ifndef _UNICODE
#define _UNICODE
#endif

#include <windows.h>
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>
#include <aclapi.h>
#include <sddl.h>
#include <iostream>
#include <string>

#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "advapi32.lib")

static const wchar_t* kFxPropsRoot = L"{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D}";
static const wchar_t* kModeEffectSlot = L",6";

static bool EnablePrivilege(LPCWSTR privName) {
    HANDLE hToken = NULL;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &hToken)) return false;

    TOKEN_PRIVILEGES tp;
    tp.PrivilegeCount = 1;
    tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
    if (!LookupPrivilegeValueW(NULL, privName, &tp.Privileges[0].Luid)) {
        CloseHandle(hToken);
        return false;
    }

    BOOL res = AdjustTokenPrivileges(hToken, FALSE, &tp, sizeof(tp), NULL, NULL);
    CloseHandle(hToken);
    return res;
}

static bool GrantFullAccessToKey(const std::wstring& subKeyPath) {
    EnablePrivilege(L"SeTakeOwnershipPrivilege");
    EnablePrivilege(L"SeRestorePrivilege");
    EnablePrivilege(L"SeSecurityPrivilege");

    HKEY hKey = NULL;
    LONG status = RegOpenKeyExW(HKEY_LOCAL_MACHINE, subKeyPath.c_str(), REG_OPTION_BACKUP_RESTORE, WRITE_OWNER, &hKey);
    if (status == ERROR_SUCCESS && hKey) {
        PSID pAdminSid = NULL;
        SID_IDENTIFIER_AUTHORITY ntAuth = SECURITY_NT_AUTHORITY;
        if (AllocateAndInitializeSid(&ntAuth, 2, SECURITY_BUILTIN_DOMAIN_RID, DOMAIN_ALIAS_RID_ADMINS, 0, 0, 0, 0, 0, 0, &pAdminSid)) {
            SetSecurityInfo(hKey, SE_REGISTRY_KEY, OWNER_SECURITY_INFORMATION, pAdminSid, NULL, NULL, NULL);
            FreeSid(pAdminSid);
        }
        RegCloseKey(hKey);
    }

    std::wstring fullPath = L"MACHINE\\" + subKeyPath;
    PSECURITY_DESCRIPTOR pSD = NULL;
    if (ConvertStringSecurityDescriptorToSecurityDescriptorW(L"D:P(A;CI;GA;;;BA)(A;CI;GA;;;SY)(A;CI;GR;;;LS)(A;CI;GR;;;IU)", SDDL_REVISION_1, &pSD, NULL)) {
        PACL pDacl = NULL;
        BOOL daclPresent = FALSE, daclDefaulted = FALSE;
        if (GetSecurityDescriptorDacl(pSD, &daclPresent, &pDacl, &daclDefaulted)) {
            SetNamedSecurityInfoW((LPWSTR)fullPath.c_str(), SE_REGISTRY_KEY, DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION, NULL, NULL, pDacl, NULL);
        }
        LocalFree(pSD);
    }
    return true;
}

int main() {
    std::wcout << L"=== Diviora Clean Mode Effect Injector ===" << std::endl;

    HRESULT hr = CoInitialize(NULL);
    if (FAILED(hr)) return 1;

    IMMDeviceEnumerator* pEnum = NULL;
    hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), NULL, CLSCTX_ALL, __uuidof(IMMDeviceEnumerator), (void**)&pEnum);
    if (FAILED(hr) || !pEnum) { CoUninitialize(); return 1; }

    IMMDeviceCollection* pCollection = NULL;
    hr = pEnum->EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE, &pCollection);
    if (SUCCEEDED(hr) && pCollection) {
        UINT count = 0;
        pCollection->GetCount(&count);
        const wchar_t* clsid = L"{D1B100A1-8888-4444-9999-000000000001}";
        DWORD clsidBytes = (DWORD)((wcslen(clsid) + 1) * sizeof(wchar_t));

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

                if (friendlyName.find(L"Speakers") != std::wstring::npos || friendlyName.find(L"Realtek") != std::wstring::npos) {
                    std::wstring fullId = pwszId ? pwszId : L"";
                    size_t lastOpen = fullId.rfind(L"{");
                    size_t lastClose = fullId.rfind(L"}");
                    std::wstring guidStr;
                    if (lastOpen != std::wstring::npos && lastClose != std::wstring::npos && lastClose > lastOpen) {
                        guidStr = fullId.substr(lastOpen, lastClose - lastOpen + 1);
                    }

                    if (!guidStr.empty()) {
                        std::wstring epSubKey = L"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\MMDevices\\Audio\\Render\\" + guidStr;
                        std::wstring fxSubKey = epSubKey + L"\\FxProperties";

                        GrantFullAccessToKey(epSubKey);
                        GrantFullAccessToKey(fxSubKey);

                        std::wstring modeSlot = std::wstring(kFxPropsRoot) + kModeEffectSlot;

                        HKEY hFx = NULL;
                        DWORD disp = 0;
                        if (RegCreateKeyExW(HKEY_LOCAL_MACHINE, fxSubKey.c_str(), 0, NULL, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, NULL, &hFx, &disp) == ERROR_SUCCESS) {
                            RegSetValueExW(hFx, modeSlot.c_str(), 0, REG_SZ, (const BYTE*)clsid, clsidBytes);
                            RegCloseKey(hFx);
                            std::wcout << L"  [+] Injected Mode APO into: " << friendlyName << std::endl;
                        }
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