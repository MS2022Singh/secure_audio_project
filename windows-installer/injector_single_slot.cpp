// Diviora endpoint binder (stream-effect slot only).
//
//   injector_single_slot.exe                       bind DivioraAPO to the default speakers
//   injector_single_slot.exe --print-guid          print the default speakers' endpoint GUID
//   injector_single_slot.exe --dump [guid]         read-only: list FxProperties values + owner/DACL
//   injector_single_slot.exe --write <guid> <name> <sz|dword|delete> [data]
//                                                  set/delete one FxProperties value (used by restore)
//
// FxProperties keys are not writable by Administrators by default. Like Equalizer APO's
// configurator, we temporarily ADD an Administrators write entry to the existing
// permissions (taking ownership only if needed), write the value(s), and then restore
// the original owner and permissions exactly. Existing entries (Audiosrv,
// AudioEndpointBuilder, TrustedInstaller, ...) are never removed.
#include <windows.h>
#include <mmdeviceapi.h>
#include <aclapi.h>
#include <sddl.h>
#include <iostream>
#include <string>
#include <vector>

#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "advapi32.lib")

static const wchar_t* kFxPropsRoot = L"{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D}";
static const wchar_t* kStreamEffectSlot = L",5";
static const wchar_t* kCompositeStreamSlot = L",13";   // PKEY_CompositeFX_StreamEffectClsid (Win10 1703+)
static const wchar_t* kDisableSysFxName = L"{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5";
static const wchar_t* kDivioraClsid = L"{D1B100A1-8888-4444-9999-000000000001}";

// PKEY_Device_FriendlyName {a45c254e-df1c-4efd-8020-67d146a850e0},14 (local copy: no INITGUID needed).
static const PROPERTYKEY kPkeyFriendlyName =
    { { 0xa45c254e, 0xdf1c, 0x4efd, { 0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0 } }, 14 };

static std::wstring FxSubKey(const std::wstring& guid) {
    return L"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\MMDevices\\Audio\\Render\\" + guid + L"\\FxProperties";
}

// ---------------------------------------------------------------- default endpoint
static bool GetDefaultEndpoint(std::wstring& guid, std::wstring& name) {
    IMMDeviceEnumerator* pEnum = nullptr;
    if (FAILED(CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL, __uuidof(IMMDeviceEnumerator), (void**)&pEnum)) || !pEnum)
        return false;
    IMMDevice* pDevice = nullptr;
    HRESULT hr = pEnum->GetDefaultAudioEndpoint(eRender, eConsole, &pDevice);
    pEnum->Release();
    if (FAILED(hr) || !pDevice) return false;
    LPWSTR id = nullptr;
    pDevice->GetId(&id);
    std::wstring full = id ? id : L"";
    if (id) CoTaskMemFree(id);
    IPropertyStore* store = nullptr;
    name = L"Default Device";
    if (SUCCEEDED(pDevice->OpenPropertyStore(STGM_READ, &store)) && store) {
        PROPVARIANT v; PropVariantInit(&v);
        if (SUCCEEDED(store->GetValue(kPkeyFriendlyName, &v)) && v.vt == VT_LPWSTR && v.pwszVal) name = v.pwszVal;
        PropVariantClear(&v);
        store->Release();
    }
    pDevice->Release();
    size_t o = full.rfind(L'{'), c = full.rfind(L'}');
    if (o == std::wstring::npos || c == std::wstring::npos || c < o) return false;
    guid = full.substr(o, c - o + 1);
    return true;
}

// ---------------------------------------------------------------- permission guard
static bool EnablePrivilege(LPCWSTR priv) {
    HANDLE tok = nullptr;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &tok)) return false;
    TOKEN_PRIVILEGES tp{};
    tp.PrivilegeCount = 1;
    tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
    bool ok = LookupPrivilegeValueW(nullptr, priv, &tp.Privileges[0].Luid) &&
              AdjustTokenPrivileges(tok, FALSE, &tp, sizeof(tp), nullptr, nullptr) &&
              GetLastError() == ERROR_SUCCESS;
    CloseHandle(tok);
    return ok;
}

struct AccessGuard {
    std::wstring objName;            // "MACHINE\\SOFTWARE\\..."
    PSECURITY_DESCRIPTOR sd = nullptr;
    PSID owner = nullptr;
    PACL dacl = nullptr;
    bool daclProtected = false;
    bool ownerChanged = false;
    bool daclChanged = false;

    ~AccessGuard() { Restore(); if (sd) LocalFree(sd); }

    void Restore() {
        if (daclChanged) {
            DWORD flags = DACL_SECURITY_INFORMATION |
                (daclProtected ? PROTECTED_DACL_SECURITY_INFORMATION : UNPROTECTED_DACL_SECURITY_INFORMATION);
            DWORD rc = SetNamedSecurityInfoW(&objName[0], SE_REGISTRY_KEY, flags, nullptr, nullptr, dacl, nullptr);
            if (rc != ERROR_SUCCESS) std::wcerr << L"[!] Could not restore permissions (err=" << rc << L")" << std::endl;
            daclChanged = false;
        }
        if (ownerChanged) {
            DWORD rc = SetNamedSecurityInfoW(&objName[0], SE_REGISTRY_KEY, OWNER_SECURITY_INFORMATION, owner, nullptr, nullptr, nullptr);
            if (rc != ERROR_SUCCESS) std::wcerr << L"[!] Could not restore owner (err=" << rc << L")" << std::endl;
            ownerChanged = false;
        }
    }
};

// Open FxProperties for writing; if denied, add a temporary Administrators entry.
static LONG OpenFxForWrite(const std::wstring& subKey, AccessGuard& g, HKEY* out) {
    LONG rc = RegOpenKeyExW(HKEY_LOCAL_MACHINE, subKey.c_str(), 0, KEY_SET_VALUE | KEY_QUERY_VALUE, out);
    if (rc != ERROR_ACCESS_DENIED) return rc;

    g.objName = L"MACHINE\\" + subKey;
    if (GetNamedSecurityInfoW(g.objName.c_str(), SE_REGISTRY_KEY, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                              &g.owner, nullptr, &g.dacl, nullptr, &g.sd) != ERROR_SUCCESS)
        return ERROR_ACCESS_DENIED;
    SECURITY_DESCRIPTOR_CONTROL ctl = 0; DWORD rev = 0;
    GetSecurityDescriptorControl(g.sd, &ctl, &rev);
    g.daclProtected = (ctl & SE_DACL_PROTECTED) != 0;

    EnablePrivilege(SE_TAKE_OWNERSHIP_NAME);
    EnablePrivilege(SE_RESTORE_NAME);   // needed to give ownership back to SYSTEM/TrustedInstaller

    BYTE adminsBuf[SECURITY_MAX_SID_SIZE]; DWORD adminsLen = sizeof(adminsBuf);
    if (!CreateWellKnownSid(WinBuiltinAdministratorsSid, nullptr, adminsBuf, &adminsLen)) return ERROR_ACCESS_DENIED;
    PSID admins = adminsBuf;

    EXPLICIT_ACCESSW ea{};
    ea.grfAccessPermissions = KEY_SET_VALUE | KEY_QUERY_VALUE;
    ea.grfAccessMode = GRANT_ACCESS;
    ea.grfInheritance = NO_INHERITANCE;
    ea.Trustee.TrusteeForm = TRUSTEE_IS_SID;
    ea.Trustee.TrusteeType = TRUSTEE_IS_GROUP;
    ea.Trustee.ptstrName = (LPWSTR)admins;
    PACL merged = nullptr;
    if (SetEntriesInAclW(1, &ea, g.dacl, &merged) != ERROR_SUCCESS) return ERROR_ACCESS_DENIED;

    DWORD flags = DACL_SECURITY_INFORMATION |
        (g.daclProtected ? PROTECTED_DACL_SECURITY_INFORMATION : UNPROTECTED_DACL_SECURITY_INFORMATION);
    DWORD s = SetNamedSecurityInfoW(&g.objName[0], SE_REGISTRY_KEY, flags, nullptr, nullptr, merged, nullptr);
    if (s == ERROR_ACCESS_DENIED) {
        // Need WRITE_DAC: take ownership first (restored afterwards).
        if (SetNamedSecurityInfoW(&g.objName[0], SE_REGISTRY_KEY, OWNER_SECURITY_INFORMATION, admins, nullptr, nullptr, nullptr) == ERROR_SUCCESS) {
            g.ownerChanged = true;
            s = SetNamedSecurityInfoW(&g.objName[0], SE_REGISTRY_KEY, flags, nullptr, nullptr, merged, nullptr);
        }
    }
    LocalFree(merged);
    if (s != ERROR_SUCCESS) { g.Restore(); return ERROR_ACCESS_DENIED; }
    g.daclChanged = true;
    return RegOpenKeyExW(HKEY_LOCAL_MACHINE, subKey.c_str(), 0, KEY_SET_VALUE | KEY_QUERY_VALUE, out);
}

// ---------------------------------------------------------------- modes
static int Dump(const std::wstring& guid) {
    std::wstring sub = FxSubKey(guid);
    std::wcout << L"Endpoint " << guid << L"\nKey HKLM\\" << sub << std::endl;
    PSECURITY_DESCRIPTOR sd = nullptr;
    if (GetNamedSecurityInfoW((L"MACHINE\\" + sub).c_str(), SE_REGISTRY_KEY, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                              nullptr, nullptr, nullptr, nullptr, &sd) == ERROR_SUCCESS) {
        LPWSTR sddl = nullptr;
        if (ConvertSecurityDescriptorToStringSecurityDescriptorW(sd, SDDL_REVISION_1, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION, &sddl, nullptr)) {
            std::wcout << L"Security: " << sddl << std::endl;
            LocalFree(sddl);
        }
        LocalFree(sd);
    }
    HKEY k = nullptr;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, sub.c_str(), 0, KEY_QUERY_VALUE, &k) != ERROR_SUCCESS) {
        std::wcerr << L"[-] FxProperties not readable or missing." << std::endl;
        return 1;
    }
    for (DWORD i = 0;; ++i) {
        wchar_t name[256]; DWORD nameLen = 256, type = 0; BYTE data[2048]; DWORD dataLen = sizeof(data);
        LONG rc = RegEnumValueW(k, i, name, &nameLen, nullptr, &type, data, &dataLen);
        if (rc == ERROR_NO_MORE_ITEMS) break;
        if (rc != ERROR_SUCCESS) continue;
        std::wcout << L"  " << name << L" = ";
        if (type == REG_SZ || type == REG_EXPAND_SZ) std::wcout << reinterpret_cast<wchar_t*>(data);
        else if (type == REG_DWORD) std::wcout << L"dword:" << *reinterpret_cast<DWORD*>(data);
        else if (type == REG_MULTI_SZ) { for (wchar_t* p = (wchar_t*)data; *p; p += wcslen(p) + 1) std::wcout << p << L" | "; }
        else std::wcout << L"(type " << type << L", " << dataLen << L" bytes)";
        std::wcout << std::endl;
    }
    RegCloseKey(k);
    return 0;
}

static bool HasValue(const std::wstring& sub, const std::wstring& name) {
    HKEY k = nullptr;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, sub.c_str(), 0, KEY_QUERY_VALUE, &k) != ERROR_SUCCESS) return false;
    bool has = RegQueryValueExW(k, name.c_str(), nullptr, nullptr, nullptr, nullptr) == ERROR_SUCCESS;
    RegCloseKey(k);
    return has;
}

static int WriteOne(const std::wstring& guid, const std::wstring& name, const std::wstring& kind, const std::wstring& data) {
    std::wstring sub = FxSubKey(guid);
    AccessGuard guard;
    HKEY k = nullptr;
    LONG rc = OpenFxForWrite(sub, guard, &k);
    if (rc != ERROR_SUCCESS) { std::wcerr << L"[-] Cannot open FxProperties for writing (err=" << rc << L")." << std::endl; return 1; }
    if (kind == L"delete") {
        rc = RegDeleteValueW(k, name.c_str());
        if (rc == ERROR_FILE_NOT_FOUND) rc = ERROR_SUCCESS;
    } else if (kind == L"dword") {
        DWORD v = static_cast<DWORD>(wcstoul(data.c_str(), nullptr, 10));
        rc = RegSetValueExW(k, name.c_str(), 0, REG_DWORD, (const BYTE*)&v, sizeof(v));
    } else if (kind == L"sz") {
        rc = RegSetValueExW(k, name.c_str(), 0, REG_SZ, (const BYTE*)data.c_str(), (DWORD)((data.size() + 1) * sizeof(wchar_t)));
    } else {
        rc = ERROR_INVALID_PARAMETER;
    }
    RegCloseKey(k);
    guard.Restore();
    std::wcout << (rc == ERROR_SUCCESS ? L"  [+] " : L"  [-] ") << kind << L" " << name << (rc == ERROR_SUCCESS ? L"" : L" FAILED") << std::endl;
    return rc == ERROR_SUCCESS ? 0 : 1;
}

static int Bind() {
    std::wcout << L"=== Diviora Single-Slot Injector (StreamEffect only) ===" << std::endl;
    std::wstring guid, name;
    if (!GetDefaultEndpoint(guid, name)) {
        std::wcerr << L"[-] Could not get default render endpoint (is the Windows Audio service running?). No changes made." << std::endl;
        return 1;
    }
    std::wstring sub = FxSubKey(guid);
    if (HasValue(sub, std::wstring(kFxPropsRoot) + kCompositeStreamSlot)) {
        std::wcerr << L"[-] " << name << L" uses composite effect slots ({d04e05a6-...},13); the ,5 slot would be ignored.\n"
                      L"    No changes made. Send the output of: injector_single_slot.exe --dump" << std::endl;
        return 2;
    }
    AccessGuard guard;
    HKEY k = nullptr;
    LONG rc = OpenFxForWrite(sub, guard, &k);
    if (rc != ERROR_SUCCESS) {
        std::wcerr << L"[-] Cannot open FxProperties for writing (err=" << rc << L"). No changes made." << std::endl;
        return 1;
    }
    std::wstring slot = std::wstring(kFxPropsRoot) + kStreamEffectSlot;
    LONG r1 = RegSetValueExW(k, slot.c_str(), 0, REG_SZ, (const BYTE*)kDivioraClsid, (DWORD)((wcslen(kDivioraClsid) + 1) * sizeof(wchar_t)));
    DWORD zero = 0;
    LONG r2 = RegSetValueExW(k, kDisableSysFxName, 0, REG_DWORD, (const BYTE*)&zero, sizeof(zero));
    RegCloseKey(k);
    guard.Restore();   // original owner and permissions back
    if (r1 != ERROR_SUCCESS || r2 != ERROR_SUCCESS) {
        std::wcerr << L"[-] Value write failed (r1=" << r1 << L" r2=" << r2 << L")." << std::endl;
        return 1;
    }
    std::wcout << L"[+] Bound (StreamEffect only) to: " << name << L" [" << guid << L"]" << std::endl;
    std::wcout << L"[+] Original registry permissions restored." << std::endl;
    return 0;
}

int wmain(int argc, wchar_t** argv) {
    if (FAILED(CoInitialize(nullptr))) return 1;
    int rc = 1;
    std::wstring mode = argc > 1 ? argv[1] : L"";
    if (mode == L"--print-guid") {
        std::wstring guid, name;
        if (GetDefaultEndpoint(guid, name)) { std::wcout << guid << std::endl; rc = 0; }
    } else if (mode == L"--dump") {
        std::wstring guid = argc > 2 ? argv[2] : L"", name;
        if (guid.empty() && !GetDefaultEndpoint(guid, name)) std::wcerr << L"[-] No default endpoint." << std::endl;
        else rc = Dump(guid);
    } else if (mode == L"--write" && argc >= 5) {
        rc = WriteOne(argv[2], argv[3], argv[4], argc > 5 ? argv[5] : L"");
    } else if (mode.empty()) {
        rc = Bind();
    } else {
        std::wcerr << L"usage: injector_single_slot.exe [--print-guid | --dump [guid] | --write <guid> <name> <sz|dword|delete> [data]]" << std::endl;
    }
    CoUninitialize();
    return rc;
}
