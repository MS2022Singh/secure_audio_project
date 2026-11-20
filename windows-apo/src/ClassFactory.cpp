#include "DivioraAPO.h"
#include <olectl.h>  // SELFREG_E_CLASS
#include <new>

static const wchar_t kClsidStr[] = L"{D1B100A1-8888-4444-9999-000000000001}";

class DivioraClassFactory : public IClassFactory {
public:
    DivioraClassFactory() : m_cRef(1) {}
    virtual ~DivioraClassFactory() {}

    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER;
        if (riid == IID_IUnknown || riid == IID_IClassFactory) {
            *ppv = static_cast<IClassFactory*>(this);
            AddRef();
            return S_OK;
        }
        *ppv = nullptr;
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override {
        ULONG r = InterlockedDecrement(&m_cRef);
        if (r == 0) delete this;
        return r;
    }
    STDMETHODIMP CreateInstance(IUnknown* pUnkOuter, REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (pUnkOuter) return CLASS_E_NOAGGREGATION;
        DivioraAPO* apo = new (std::nothrow) DivioraAPO();
        if (!apo) return E_OUTOFMEMORY;
        HRESULT hr = apo->QueryInterface(riid, ppv);
        apo->Release();
        return hr;
    }
    STDMETHODIMP LockServer(BOOL) override { return S_OK; }

private:
    volatile LONG m_cRef;
};

STDAPI DllGetClassObject(REFCLSID rclsid, REFIID riid, LPVOID* ppv) {
    if (!ppv) return E_POINTER;
    *ppv = nullptr;
    if (!IsEqualCLSID(rclsid, CLSID_DivioraAPO)) return CLASS_E_CLASSNOTAVAILABLE;
    DivioraClassFactory* f = new (std::nothrow) DivioraClassFactory();
    if (!f) return E_OUTOFMEMORY;
    HRESULT hr = f->QueryInterface(riid, ppv);
    f->Release();
    return hr;
}

// The config watcher thread lives inside this DLL, so it must never be unloaded.
STDAPI DllCanUnloadNow() { return S_FALSE; }

// ---------------- Registration (regsvr32) ----------------
static LONG SetSz(HKEY k, const wchar_t* name, const wchar_t* val) {
    return RegSetValueExW(k, name, 0, REG_SZ, reinterpret_cast<const BYTE*>(val),
                          static_cast<DWORD>((wcslen(val) + 1) * sizeof(wchar_t)));
}
static LONG SetDw(HKEY k, const wchar_t* name, DWORD val) {
    return RegSetValueExW(k, name, 0, REG_DWORD, reinterpret_cast<const BYTE*>(&val), sizeof(val));
}
static HKEY CreateKey(const std::wstring& path) {
    HKEY k = nullptr;
    if (RegCreateKeyExW(HKEY_LOCAL_MACHINE, path.c_str(), 0, nullptr, 0, KEY_WRITE, nullptr, &k, nullptr) != ERROR_SUCCESS)
        return nullptr;
    return k;
}

STDAPI DllRegisterServer() {
    wchar_t module[MAX_PATH];
    HMODULE self = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            reinterpret_cast<LPCWSTR>(&DllRegisterServer), &self) ||
        !GetModuleFileNameW(self, module, MAX_PATH))
        return E_FAIL;

    const std::wstring clsidKey = std::wstring(L"SOFTWARE\\Classes\\CLSID\\") + kClsidStr;
    HKEY k = CreateKey(clsidKey);
    if (!k) return SELFREG_E_CLASS;
    SetSz(k, nullptr, L"Diviora Endpoint APO");
    RegCloseKey(k);
    k = CreateKey(clsidKey + L"\\InprocServer32");
    if (!k) return SELFREG_E_CLASS;
    SetSz(k, nullptr, module);
    SetSz(k, L"ThreadingModel", L"Both");
    RegCloseKey(k);

    // Audio engine APO registration: the same values the Windows SDK's RegisterAPO()
    // writes, mirroring GetRegistrationProperties().
    k = CreateKey(std::wstring(L"SOFTWARE\\Classes\\AudioEngine\\AudioProcessingObjects\\") + kClsidStr);
    if (!k) return SELFREG_E_CLASS;
    SetSz(k, L"FriendlyName", L"Diviora Endpoint APO");
    SetSz(k, L"Copyright", L"Diviora");
    SetDw(k, L"MajorVersion", 1);
    SetDw(k, L"MinorVersion", 1);
    SetDw(k, L"Flags", APO_FLAG_INPLACE | APO_FLAG_SAMPLESPERFRAME_MUST_MATCH |
                       APO_FLAG_FRAMESPERSECOND_MUST_MATCH | APO_FLAG_BITSPERSAMPLE_MUST_MATCH);
    SetDw(k, L"MinInputConnections", 1);
    SetDw(k, L"MaxInputConnections", 1);
    SetDw(k, L"MinOutputConnections", 1);
    SetDw(k, L"MaxOutputConnections", 1);
    SetDw(k, L"MaxInstances", 0xFFFFFFFF);
    SetDw(k, L"NumAPOInterfaces", 1);
    SetSz(k, L"APOInterface0", L"{FD7F2B29-24D0-4B5C-B177-592C39F9CA10}"); // IID_IAudioProcessingObject
    RegCloseKey(k);
    return S_OK;
}

STDAPI DllUnregisterServer() {
    RegDeleteTreeW(HKEY_LOCAL_MACHINE, (std::wstring(L"SOFTWARE\\Classes\\AudioEngine\\AudioProcessingObjects\\") + kClsidStr).c_str());
    RegDeleteKeyW(HKEY_LOCAL_MACHINE, (std::wstring(L"SOFTWARE\\Classes\\AudioEngine\\AudioProcessingObjects\\") + kClsidStr).c_str());
    RegDeleteTreeW(HKEY_LOCAL_MACHINE, (std::wstring(L"SOFTWARE\\Classes\\CLSID\\") + kClsidStr).c_str());
    RegDeleteKeyW(HKEY_LOCAL_MACHINE, (std::wstring(L"SOFTWARE\\Classes\\CLSID\\") + kClsidStr).c_str());
    return S_OK;
}
