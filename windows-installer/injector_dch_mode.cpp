#include <windows.h>
#include <mmdeviceapi.h>
#include <iostream>
#include <vector>
#include <string>

static const wchar_t* kDivioraClsid = L"{D1B100A1-8888-4444-9999-000000000001}";

void InjectMultiSz(HKEY hKey, const wchar_t* valName) {
    DWORD dwType = 0;
    DWORD cbData = 0;
    LONG status = RegQueryValueExW(hKey, valName, NULL, &dwType, NULL, &cbData);
    
    std::vector<wchar_t> buffer;
    if (status == ERROR_SUCCESS && dwType == REG_MULTI_SZ && cbData > 0) {
        buffer.resize(cbData / sizeof(wchar_t));
        RegQueryValueExW(hKey, valName, NULL, NULL, (LPBYTE)buffer.data(), &cbData);
    }

    // Check if Diviora is already in the list
    bool exists = false;
    const wchar_t* p = buffer.data();
    while (p && *p) {
        if (_wcsicmp(p, kDivioraClsid) == 0) {
            exists = true;
            break;
        }
        p += wcslen(p) + 1;
    }

    if (exists) {
        std::wcout << L"  [i] " << valName << L" already contains Diviora APO." << std::endl;
        return;
    }

    // Construct new REG_MULTI_SZ with Diviora prepended
    std::vector<wchar_t> newBuffer;
    // 1. Add Diviora
    for (const wchar_t* c = kDivioraClsid; *c; ++c) newBuffer.push_back(*c);
    newBuffer.push_back(L'\0');

    // 2. Add existing entries
    if (!buffer.empty()) {
        const wchar_t* cur = buffer.data();
        while (*cur) {
            size_t len = wcslen(cur);
            for (size_t i = 0; i < len; ++i) newBuffer.push_back(cur[i]);
            newBuffer.push_back(L'\0');
            cur += len + 1;
        }
    }
    newBuffer.push_back(L'\0'); // Final double null-terminator

    status = RegSetValueExW(hKey, valName, 0, REG_MULTI_SZ, (const BYTE*)newBuffer.data(), (DWORD)(newBuffer.size() * sizeof(wchar_t)));
    if (status == ERROR_SUCCESS) {
        std::wcout << L"  [+] Injected Diviora into " << valName << std::endl;
    } else {
        std::wcout << L"  [-] Failed to write " << valName << L", err=" << status << std::endl;
    }
}

int main() {
    std::cout << "=== Diviora Modern DCH Multi-Mode Injector ===" << std::endl;

    const wchar_t* subKey = L"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\MMDevices\\Audio\\Render\\{74e3dfaf-ea45-49ef-a4c7-eaef0bbbdc51}\\FxProperties";
    HKEY hKey = NULL;
    LONG res = RegOpenKeyExW(HKLM, subKey, 0, KEY_READ | KEY_WRITE, &hKey);
    if (res != ERROR_SUCCESS) {
        std::cerr << "[-] Failed to open FxProperties key. Error=" << res << std::endl;
        return 1;
    }

    // Inject into Streaming StreamEffect Mode arrays
    InjectMultiSz(hKey, L"{d3993a3f-99c2-4402-b5ec-a92a0367664b},5");
    InjectMultiSz(hKey, L"{d3993a3f-99c2-4402-b5ec-a92a0367664b},11");

    RegCloseKey(hKey);
    std::cout << "=== Injection Completed Successfully ===" << std::endl;
    return 0;
}