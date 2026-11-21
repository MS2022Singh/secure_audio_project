// Diviora APO self-test: loads DivioraAPO.dll in a normal process and drives it the way
// audiodg.exe does (Initialize -> LockForProcess -> APOProcess), using a temporary
// ProgramData folder. Touches no registry, no audio device and no audiodg.exe.
// Built by windows-apo\build_native_apo.bat; run: build\Release\apo_selftest.exe
#include <windows.h>
#include <wincrypt.h>
#include <mmreg.h>
#include <cmath>
#include <cstdio>
#include <string>
#include <audioenginebaseapo.h>
#pragma comment(lib, "crypt32.lib")
#pragma comment(lib, "ole32.lib")

static const CLSID CLSID_DivioraAPO = { 0xD1B100A1, 0x8888, 0x4444, { 0x99,0x99,0,0,0,0,0,1 } };
static const GUID kFloat = { 0x00000003, 0x0000, 0x0010, { 0x80,0x00,0x00,0xaa,0x00,0x38,0x9b,0x71 } };

struct FakeMediaType : IAudioMediaType {
    WAVEFORMATEXTENSIBLE wfx{};
    FakeMediaType() {
        wfx.Format.wFormatTag = WAVE_FORMAT_EXTENSIBLE; wfx.Format.nChannels = 2; wfx.Format.nSamplesPerSec = 48000;
        wfx.Format.wBitsPerSample = 32; wfx.Format.nBlockAlign = 8; wfx.Format.nAvgBytesPerSec = 48000 * 8;
        wfx.Format.cbSize = 22; wfx.Samples.wValidBitsPerSample = 32; wfx.dwChannelMask = 3; wfx.SubFormat = kFloat;
    }
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID, void** p) override { *p = this; return S_OK; }
    ULONG STDMETHODCALLTYPE AddRef() override { return 1; }
    ULONG STDMETHODCALLTYPE Release() override { return 1; }
    HRESULT STDMETHODCALLTYPE IsCompressedFormat(BOOL* b) override { *b = FALSE; return S_OK; }
    HRESULT STDMETHODCALLTYPE IsEqual(IAudioMediaType*, DWORD*) override { return E_NOTIMPL; }
    const WAVEFORMATEX* STDMETHODCALLTYPE GetAudioFormat() override { return &wfx.Format; }
    HRESULT STDMETHODCALLTYPE GetUncompressedAudioFormat(UNCOMPRESSEDAUDIOFORMAT*) override { return E_NOTIMPL; }
};

// Minimal controlling object, the way audiodg.exe aggregates APOs.
struct FakeOuter : IUnknown {
    LONG refs = 1;
    IUnknown* inner = nullptr;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** p) override {
        if (riid == IID_IUnknown) { *p = this; AddRef(); return S_OK; }
        return inner ? inner->QueryInterface(riid, p) : E_NOINTERFACE;   // delegate to the APO
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return InterlockedIncrement(&refs); }
    ULONG STDMETHODCALLTYPE Release() override { return InterlockedDecrement(&refs); }
};

static bool WriteConfig(const std::wstring& path, const std::string& text) {
    static const char ent[] = "Diviora.APO.Config.v1";
    DATA_BLOB in{ (DWORD)text.size(), (BYTE*)text.data() }, e{ (DWORD)sizeof(ent) - 1, (BYTE*)ent }, out{};
    if (!CryptProtectData(&in, L"Diviora", &e, nullptr, nullptr, CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN, &out)) return false;
    std::wstring tmp = path + L".tmp";
    HANDLE h = CreateFileW(tmp.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, 0, nullptr);
    DWORD w; UINT32 ver = 1;
    WriteFile(h, "DVRC", 4, &w, nullptr); WriteFile(h, &ver, 4, &w, nullptr); WriteFile(h, out.pbData, out.cbData, &w, nullptr);
    CloseHandle(h); LocalFree(out.pbData);
    return MoveFileExW(tmp.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING) != 0;
}

static double RunBlockDb(IAudioProcessingObjectRT* rt, double freq, int blocks) {
    static float buf[480 * 2];
    static double phase = 0;
    double sumIn = 0, sumOut = 0;
    for (int b = 0; b < blocks; ++b) {
        for (int i = 0; i < 480; ++i) { float v = (float)(0.5 * sin(phase)); phase += 2 * 3.14159265358979323846 * freq / 48000; buf[2*i] = buf[2*i+1] = v; }
        double si = 0; for (float s : buf) si += s * s;
        APO_CONNECTION_PROPERTY inP{ (UINT_PTR)buf, 480, BUFFER_VALID, 0 }, outP{ (UINT_PTR)buf, 480, BUFFER_VALID, 0 };
        APO_CONNECTION_PROPERTY* pi = &inP; APO_CONNECTION_PROPERTY* po = &outP;
        rt->APOProcess(1, &pi, 1, &po);
        double so = 0; for (float s : buf) so += s * s;
        if (b >= blocks / 2) { sumIn += si; sumOut += so; } // skip filter transient
    }
    return 10 * log10(sumOut / sumIn);
}

int main() {
    wchar_t tmpDir[MAX_PATH];
    GetTempPathW(MAX_PATH, tmpDir);
    std::wstring pd = std::wstring(tmpDir) + L"diviora_apo_selftest";
    CreateDirectoryW(pd.c_str(), nullptr);
    CreateDirectoryW((pd + L"\\Diviora").c_str(), nullptr);
    CreateDirectoryW((pd + L"\\Diviora\\apo").c_str(), nullptr);
    std::wstring cfg = pd + L"\\Diviora\\apo\\config.dvr";
    DeleteFileW(cfg.c_str());
    SetEnvironmentVariableW(L"ProgramData", pd.c_str());

    HMODULE dll = LoadLibraryW(L"DivioraAPO.dll");
    if (!dll) { printf("FAIL load %lu\n", GetLastError()); return 1; }
    auto getCO = (HRESULT(WINAPI*)(REFCLSID, REFIID, void**))GetProcAddress(dll, "DllGetClassObject");
    IClassFactory* cf = nullptr;
    if (FAILED(getCO(CLSID_DivioraAPO, IID_IClassFactory, (void**)&cf))) { printf("FAIL factory\n"); return 1; }
    IAudioProcessingObject* apo = nullptr;
    cf->CreateInstance(nullptr, __uuidof(IAudioProcessingObject), (void**)&apo);
    IAudioProcessingObjectConfiguration* conf = nullptr; IAudioProcessingObjectRT* rt = nullptr;
    apo->QueryInterface(__uuidof(IAudioProcessingObjectConfiguration), (void**)&conf);
    apo->QueryInterface(__uuidof(IAudioProcessingObjectRT), (void**)&rt);
    APO_REG_PROPERTIES* rp = nullptr; apo->GetRegistrationProperties(&rp);
    printf("registration: maxInstances=%u flags=%u\n", rp->u32MaxInstances, (unsigned)rp->Flags); CoTaskMemFree(rp);

    printf("initialize: 0x%lx\n", apo->Initialize(0, nullptr));
    FakeMediaType mt;
    APO_CONNECTION_DESCRIPTOR d{ APO_CONNECTION_BUFFER_TYPE_EXTERNAL, 0, 480, &mt, 0 };
    APO_CONNECTION_DESCRIPTOR* pd1 = &d;
    printf("lock: 0x%lx\n", conf->LockForProcess(1, &pd1, 1, &pd1));

    int fails = 0;
    auto check = [&](const char* name, double got, double want, double tol) {
        bool ok = fabs(got - want) <= tol; fails += !ok;
        printf("%-44s %8.2f dB (want %6.1f) %s\n", name, got, want, ok ? "PASS" : "FAIL");
    };
    check("no config -> pass-through @1k", RunBlockDb(rt, 1000, 40), 0, 0.01);

    WriteConfig(cfg, "Preamp: -24 dB\n"); Sleep(1500);
    check("Preamp -24 dB @1k (hot reload)", RunBlockDb(rt, 1000, 40), -24, 0.05);

    WriteConfig(cfg, "Filter: ON PK Fc 1000 Hz Gain -24 dB Q 2\n"); Sleep(1500);
    check("PK 1k -24 dB @1k (hot reload)", RunBlockDb(rt, 1000, 200), -24, 0.3);
    check("PK 1k -24 dB @100 Hz", RunBlockDb(rt, 100, 200), 0, 0.3);

    HANDLE h = CreateFileW(cfg.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, 0, nullptr);
    DWORD w; WriteFile(h, "garbage-not-encrypted", 21, &w, nullptr); CloseHandle(h); Sleep(1500);
    check("corrupt config -> pass-through (not silence)", RunBlockDb(rt, 1000, 40), 0, 0.01);

    WriteConfig(cfg, "Preamp: -6 dB\n"); Sleep(1500);
    check("valid again -> -6 dB", RunBlockDb(rt, 1000, 40), -6, 0.05);
    DeleteFileW(cfg.c_str()); Sleep(1500);
    check("config deleted -> pass-through", RunBlockDb(rt, 1000, 40), 0, 0.01);

    Sleep(1200);
    char st[1024] = {}; DWORD r = 0;
    HANDLE s = CreateFileW((pd + L"\\Diviora\\apo\\status\\apo_status.json").c_str(), GENERIC_READ, FILE_SHARE_READ|FILE_SHARE_DELETE, nullptr, OPEN_EXISTING, 0, nullptr);
    if (s != INVALID_HANDLE_VALUE) { ReadFile(s, st, sizeof(st) - 1, &r, nullptr); CloseHandle(s); }
    printf("status: %s", r ? st : "MISSING\n"); fails += !r;

    printf("unlock: 0x%lx\n", conf->UnlockForProcess());
    rt->Release(); conf->Release(); apo->Release();

    // Aggregated creation: what audiodg.exe does (CreateInstance(outer, IID_IUnknown)).
    {
        FakeOuter outer;
        HRESULT hr = cf->CreateInstance(&outer, IID_IUnknown, (void**)&outer.inner);
        bool ok = SUCCEEDED(hr) && outer.inner;
        IAudioProcessingObject* aApo = nullptr; IAudioProcessingObjectConfiguration* aConf = nullptr; IAudioProcessingObjectRT* aRt = nullptr;
        if (ok) ok = SUCCEEDED(outer.QueryInterface(__uuidof(IAudioProcessingObject), (void**)&aApo)) &&
                     SUCCEEDED(outer.QueryInterface(__uuidof(IAudioProcessingObjectConfiguration), (void**)&aConf)) &&
                     SUCCEEDED(outer.QueryInterface(__uuidof(IAudioProcessingObjectRT), (void**)&aRt));
        // Interfaces must report the outer object as their IUnknown (COM identity rule).
        IUnknown* ident = nullptr;
        if (ok) { aApo->QueryInterface(IID_IUnknown, (void**)&ident); ok = (ident == &outer); if (ident) ident->Release(); }
        if (ok) ok = SUCCEEDED(aApo->Initialize(0, nullptr)) && SUCCEEDED(aConf->LockForProcess(1, &pd1, 1, &pd1));
        if (ok) {
            WriteConfig(cfg, "Preamp: -12 dB\n"); Sleep(1500);
            check("aggregated (audiodg-style): Preamp -12 dB", RunBlockDb(aRt, 1000, 40), -12, 0.05);
            DeleteFileW(cfg.c_str()); Sleep(1500);
            aConf->UnlockForProcess();
        } else {
            printf("aggregated (audiodg-style) create/query: FAIL (hr=0x%lx)\n", hr); fails++;
        }
        if (aRt) aRt->Release();
        if (aConf) aConf->Release();
        if (aApo) aApo->Release();
        if (outer.inner) outer.inner->Release();   // final release of the inner object deletes the APO
    }
    cf->Release();
    printf("%s (%d failures)\n", fails ? "SOME CHECKS FAILED" : "ALL CHECKS PASSED", fails);
    return fails ? 1 : 0;
}
