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

static const wchar_t* kFxPropsRoot        = L"{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D}";
static const wchar_t* kStreamEffectSlot   = L",5";
static const wchar_t* kModeEffectSlot     = L",6";
static const wchar_t* kEndpointEffectSlot = L",7";
static const wchar_t* kDisableSysFxName   = L"{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5";
static const wchar_t* kBackupKeyPath      = L"SOFTWARE\\Diviora\\FxPropertyBackup";

static bool EnablePrivilege(LPCWSTR privName) {
    HANDLE hToken = NULL;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &hToken)) {
        return false;
    }

    TOKEN_PRIVILEGES tp;
    tp.PrivilegeCount = 1;
    tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
    if (!LookupPrivilegeValueW(NULL, privName, &tp.Privileges[0].Luid)) {
        CloseHandle(hToken);
        return false;
    }

    BOOL res = AdjustTokenPrivileges(hToken, FALSE, &tp, sizeof(tp), NULL, NULL);
    DWORD err = GetLastError();
    CloseHandle(hToken);

    if (!res || err == ERROR_NOT_ALL_ASSIGNED) {
        std::wcerr << L"  [-] Failed to enable privilege " << privName << L" (err=" << err << L")" << std::endl;
        return false;
    }
    return true;
}

static bool GrantFullAccessToKey(const std::wstring& subKeyPath) {
    EnablePrivilege(L"SeTakeOwnershipPrivilege");
    EnablePrivilege(L"SeRestorePrivilege");
    EnablePrivilege(L"SeSecurityPrivilege");

    HKEY hKey = NULL;
    LONG status = RegOpenKeyExW(HKEY_LOCAL_MACHINE, subKeyPath.c_str(),
                                REG_OPTION_BACKUP_RESTORE, WRITE_OWNER, &hKey);
    if (status == ERROR_SUCCESS && hKey) {
        PSID pAdminSid = NULL;
        SID_IDENTIFIER_AUTHORITY ntAuth = SECURITY_NT_AUTHORITY;
        if (AllocateAndInitializeSid(&ntAuth, 2, SECURITY_BUILTIN_DOMAIN_RID,
                                     DOMAIN_ALIAS_RID_ADMINS, 0, 0, 0, 0, 0, 0, &pAdminSid)) {
            DWORD ownerResult = SetSecurityInfo(hKey, SE_REGISTRY_KEY,
                                                OWNER_SECURITY_INFORMATION,
                                                pAdminSid, NULL, NULL, NULL);
            if (ownerResult != ERROR_SUCCESS) {
                std::wcerr << L"  [-] SetSecurityInfo(owner) failed on " << subKeyPath << L" (err=" << ownerResult << L")" << std::endl;
            }
            FreeSid(pAdminSid);
        }
        RegCloseKey(hKey);
    }

    std::wstring fullPath = L"MACHINE\\" + subKeyPath;
    PSECURITY_DESCRIPTOR pSD = NULL;
    bool ok = false;

    if (ConvertStringSecurityDescriptorToSecurityDescriptorW(
            L"D:P(A;CI;GA;;;BA)(A;CI;GA;;;SY)(A;CI;GR;;;LS)(A;CI;GR;;;IU)", SDDL_REVISION_1, &pSD, NULL)) {
        PACL pDacl = NULL;
        BOOL daclPresent = FALSE, daclDefaulted = FALSE;
        if (GetSecurityDescriptorDacl(pSD, &daclPresent, &pDacl, &daclDefaulted)) {
            DWORD daclResult = SetNamedSecurityInfoW(
                (LPWSTR)fullPath.c_str(), SE_REGISTRY_KEY,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                NULL, NULL, pDacl, NULL);
            if (daclResult == ERROR_SUCCESS) {
                ok = true;
            } else {
                std::wcerr << L"  [-] SetNamedSecurityInfoW(DACL) failed on " << subKeyPath << L" (err=" << daclResult << L")" << std::endl;
            }
        }
        LocalFree(pSD);
    }
    return ok;
}

static void BackupExistingValue(const std::wstring& fxSubKey, const std::wstring& guidStr, const std::wstring& slotName) {
    HKEY hSrc = NULL;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, fxSubKey.c_str(), 0, KEY_READ, &hSrc) != ERROR_SUCCESS) {
        return;
    }

    wchar_t buf[512] = {0};
    DWORD bufBytes = sizeof(buf);
    DWORD type = 0;
    LONG r = RegQueryValueExW(hSrc, slotName.c_str(), NULL, &type, (LPBYTE)buf, &bufBytes);
    RegCloseKey(hSrc);

    if (r != ERROR_SUCCESS || type != REG_SZ) {
        return;
    }

    std::wcout << L"  [i] Existing value in " << slotName << L" was: " << buf << L" - backing up..." << std::endl;

    std::wstring backupPath = std::wstring(kBackupKeyPath) + L"\\" + guidStr;
    HKEY hDst = NULL;
    if (RegCreateKeyExW(HKEY_LOCAL_MACHINE, backupPath.c_str(), 0, NULL,
                        REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, NULL, &hDst, NULL) == ERROR_SUCCESS) {
        RegSetValueExW(hDst, slotName.c_str(), 0, REG_SZ, (const BYTE*)buf, (DWORD)((wcslen(buf) + 1) * sizeof(wchar_t)));
        RegCloseKey(hDst);
    }
}

int main() {
    std::wcout << L"=== Diviora Native Win32 APO Registry Injector ===" << std::endl;

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
                std::wstring guidStr;
                if (lastOpen != std::wstring::npos && lastClose != std::wstring::npos && lastClose > lastOpen) {
                    guidStr = fullId.substr(lastOpen, lastClose - lastOpen + 1);
                }

                if (!guidStr.empty()) {
                    std::wstring epSubKey    = L"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\MMDevices\\Audio\\Render\\" + guidStr;
                    std::wstring propsSubKey = epSubKey + L"\\Properties";
                    std::wstring fxSubKey    = epSubKey + L"\\FxProperties";

                    GrantFullAccessToKey(epSubKey);
                    GrantFullAccessToKey(propsSubKey);
                    GrantFullAccessToKey(fxSubKey);

                    std::wstring streamSlot   = std::wstring(kFxPropsRoot) + kStreamEffectSlot;
                    std::wstring modeSlot     = std::wstring(kFxPropsRoot) + kModeEffectSlot;
                    std::wstring endpointSlot = std::wstring(kFxPropsRoot) + kEndpointEffectSlot;

                    BackupExistingValue(fxSubKey, guidStr, streamSlot);
                    BackupExistingValue(fxSubKey, guidStr, modeSlot);

                    // 1. Enable System Effects under Properties
                    HKEY hProps = NULL;
                    if (RegCreateKeyExW(HKEY_LOCAL_MACHINE, propsSubKey.c_str(), 0, NULL,
                                        REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, NULL, &hProps, NULL) == ERROR_SUCCESS) {
                        RegSetValueExW(hProps, kDisableSysFxName, 0, REG_DWORD, (const BYTE*)&disableSysFx, sizeof(DWORD));
                        RegCloseKey(hProps);
                    }

                    // 2. Inject APO into FxProperties
                    HKEY hFx = NULL;
                    if (RegCreateKeyExW(HKEY_LOCAL_MACHINE, fxSubKey.c_str(), 0, NULL,
                                        REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, NULL, &hFx, NULL) == ERROR_SUCCESS) {
                        LONG r1 = RegSetValueExW(hFx, streamSlot.c_str(), 0, REG_SZ, (const BYTE*)clsid, clsidBytes);
                        LONG r2 = RegSetValueExW(hFx, modeSlot.c_str(), 0, REG_SZ, (const BYTE*)clsid, clsidBytes);
                        LONG r3 = RegSetValueExW(hFx, endpointSlot.c_str(), 0, REG_SZ, (const BYTE*)clsid, clsidBytes);
                        RegCloseKey(hFx);

                        if (r1 == ERROR_SUCCESS && r2 == ERROR_SUCCESS && r3 == ERROR_SUCCESS) {
                            std::wcout << L"  [+] Injected & enabled: " << friendlyName << L" [" << guidStr << L"]" << std::endl;
                        } else {
                            std::wcerr << L"  [-] Failed setting FxProperties on: " << friendlyName << std::endl;
                        }
                    } else {
                        std::wcerr << L"  [-] Failed to create/open FxProperties on: " << friendlyName << std::endl;
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

    std::wcout << L"\n=== Injection complete. ===" << std::endl;
    return 0;
}