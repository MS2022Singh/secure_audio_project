#include "ConfigManager.h"
#include "DivioraAPO.h"
#include <wincrypt.h>
#include <cstdio>
#include <thread>

#pragma comment(lib, "crypt32.lib")

// File format of config.dvr (written by the Diviora app, src-tauri/src/apo_config.rs):
//   bytes 0..4  "DVRC"
//   bytes 4..8  little-endian u32 format version (1)
//   bytes 8..   DPAPI blob (CryptProtectData, CRYPTPROTECT_LOCAL_MACHINE, entropy below)
// The decrypted payload is the Equalizer APO configuration text in UTF-8.
static const char kMagic[4] = { 'D', 'V', 'R', 'C' };
static const UINT32 kFormatVersion = 1;
static const char kEntropy[] = "Diviora.APO.Config.v1";
static const DWORD kMaxConfigBytes = 4 * 1024 * 1024;

DivioraCounters& DivioraStats() {
    static DivioraCounters s;
    return s;
}

ConfigManager& ConfigManager::Instance() {
    static ConfigManager s;
    return s;
}

static std::wstring ProgramDataDir() {
    wchar_t buf[MAX_PATH];
    DWORD n = GetEnvironmentVariableW(L"ProgramData", buf, MAX_PATH);
    if (n == 0 || n >= MAX_PATH) return L"C:\\ProgramData";
    return std::wstring(buf, n);
}

void ConfigManager::Register(DivioraAPO* apo) {
    {
        std::lock_guard<std::mutex> g(m_mutex);
        m_instances.push_back(apo);
    }
    DivioraStats().instances++;
    EnsureThread();
}

void ConfigManager::Unregister(DivioraAPO* apo) {
    std::lock_guard<std::mutex> g(m_mutex);
    for (size_t i = 0; i < m_instances.size(); ++i) {
        if (m_instances[i] == apo) {
            m_instances.erase(m_instances.begin() + i);
            DivioraStats().instances--;
            break;
        }
    }
}

void ConfigManager::EnsureThread() {
    bool expected = false;
    if (!m_threadStarted.compare_exchange_strong(expected, true)) return;
    m_baseDir    = ProgramDataDir() + L"\\Diviora\\apo";
    m_configPath = m_baseDir + L"\\config.dvr";
    m_statusDir  = m_baseDir + L"\\status";
    m_statusPath = m_statusDir + L"\\apo_status.json";
    // The DLL is never unloaded (DllCanUnloadNow returns S_FALSE), so this thread
    // lives as long as audiodg.exe and is never joined.
    std::thread([this] { ThreadMain(); }).detach();
}

void ConfigManager::ThreadMain() {
    HANDLE hChange = INVALID_HANDLE_VALUE;
    for (;;) {
        if (hChange == INVALID_HANDLE_VALUE) {
            hChange = FindFirstChangeNotificationW(m_baseDir.c_str(), FALSE,
                FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_SIZE);
        }
        bool changed = ReloadIfChanged();
        {
            std::lock_guard<std::mutex> g(m_mutex);
            for (DivioraAPO* apo : m_instances) {
                apo->ReleaseRetired();                 // free engines the audio thread swapped out
                if (changed) apo->OnConfigChanged(m_text);
            }
        }
        WriteStatus();

        if (hChange != INVALID_HANDLE_VALUE) {
            // Wake on a change in the folder, or after 1 s for status and polling fallback.
            if (WaitForSingleObject(hChange, 1000) == WAIT_OBJECT_0) {
                Sleep(100); // let the writer finish its atomic rename
                if (!FindNextChangeNotification(hChange)) {
                    FindCloseChangeNotification(hChange);
                    hChange = INVALID_HANDLE_VALUE;
                }
            }
        } else {
            Sleep(1000); // folder missing or not accessible: poll
        }
    }
}

// Reads, validates and decrypts config.dvr when its size or timestamp changed.
bool ConfigManager::ReloadIfChanged() {
    WIN32_FILE_ATTRIBUTE_DATA fad{};
    if (!GetFileAttributesExW(m_configPath.c_str(), GetFileExInfoStandard, &fad)) {
        DWORD err = GetLastError();
        bool hadConfig = m_lastSize != ~0ULL;
        m_lastSize = ~0ULL;
        m_lastError = (err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND)
            ? L"no config.dvr (pass-through)" : L"cannot read config.dvr attributes (error " + std::to_wstring(err) + L")";
        if (hadConfig) {
            std::lock_guard<std::mutex> g(m_mutex);
            m_text.clear();
            m_revision++;
            return true; // config removed -> pass-through
        }
        return false;
    }
    ULONGLONG size = (static_cast<ULONGLONG>(fad.nFileSizeHigh) << 32) | fad.nFileSizeLow;
    if (size == m_lastSize && CompareFileTime(&fad.ftLastWriteTime, &m_lastWrite) == 0) return false;

    std::string newText;
    std::wstring error;
    HANDLE h = CreateFileW(m_configPath.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_DELETE, nullptr,
                           OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE) {
        error = L"cannot open config.dvr (error " + std::to_wstring(GetLastError()) + L"; check folder permissions)";
    } else if (size < 9 || size > kMaxConfigBytes) {
        error = L"config.dvr has invalid size";
        CloseHandle(h);
    } else {
        std::string raw(static_cast<size_t>(size), '\0');
        DWORD got = 0;
        BOOL ok = ReadFile(h, &raw[0], static_cast<DWORD>(size), &got, nullptr);
        CloseHandle(h);
        UINT32 ver = 0;
        if (ok && got == size) memcpy(&ver, raw.data() + 4, 4);
        if (!ok || got != size) {
            error = L"short read on config.dvr";
        } else if (memcmp(raw.data(), kMagic, 4) != 0 || ver != kFormatVersion) {
            error = L"config.dvr is not a Diviora v1 file";
        } else {
            DATA_BLOB in{ static_cast<DWORD>(size - 8), reinterpret_cast<BYTE*>(&raw[8]) };
            DATA_BLOB ent{ static_cast<DWORD>(sizeof(kEntropy) - 1), (BYTE*)kEntropy };
            DATA_BLOB out{};
            if (CryptUnprotectData(&in, nullptr, &ent, nullptr, nullptr, CRYPTPROTECT_UI_FORBIDDEN, &out)) {
                newText.assign(reinterpret_cast<char*>(out.pbData), out.cbData);
                SecureZeroMemory(out.pbData, out.cbData);
                LocalFree(out.pbData);
            } else {
                error = L"DPAPI decrypt failed (error " + std::to_wstring(GetLastError()) + L"); file written on another PC?";
            }
        }
        SecureZeroMemory(&raw[0], raw.size());
    }

    m_lastSize = size;
    m_lastWrite = fad.ftLastWriteTime;
    m_lastError = error.empty() ? L"none" : error;
    std::lock_guard<std::mutex> g(m_mutex);
    // On any error keep pass-through (empty text): never silence, never stale filters.
    if (!m_text.empty()) SecureZeroMemory(&m_text[0], m_text.size());
    m_text = error.empty() ? newText : std::string();
    if (!newText.empty()) SecureZeroMemory(&newText[0], newText.size());
    m_revision++;
    return true;
}

static std::string Narrow(const std::wstring& w) {
    std::string s;
    for (wchar_t c : w) {
        if (c == L'"' || c == L'\\') s.push_back('\\');
        s.push_back((c >= 32 && c < 127) ? static_cast<char>(c) : '?');
    }
    return s;
}

void ConfigManager::WriteStatus() {
    CreateDirectoryW(m_statusDir.c_str(), nullptr); // usually created by the installer
    auto& st = DivioraStats();
    wchar_t exe[MAX_PATH] = L"?";
    GetModuleFileNameW(nullptr, exe, MAX_PATH);
    const wchar_t* exeName = wcsrchr(exe, L'\\');
    exeName = exeName ? exeName + 1 : exe;
    bool hasConfig;
    { std::lock_guard<std::mutex> g(m_mutex); hasConfig = !m_text.empty(); }

    char json[1200];
    int n = snprintf(json, sizeof(json),
        "{\"writer\":\"DivioraAPO.dll\",\"pid\":%lu,\"process\":\"%s\",\"tick_ms\":%llu,"
        "\"instances\":%ld,\"locked\":%ld,\"process_calls\":%lld,\"processed_frames\":%lld,"
        "\"filtered_frames\":%lld,\"sample_rate\":%ld,\"channels\":%ld,\"non_float_streams\":%ld,"
        "\"config_revision\":%lld,\"config_loaded\":%s,\"last_error\":\"%s\",\"status_dir_error\":%lu}\n",
        GetCurrentProcessId(), Narrow(exeName).c_str(), GetTickCount64(),
        st.instances.load(), st.locked.load(), st.process_calls.load(), st.processed_frames.load(),
        st.filtered_frames.load(), st.last_sample_rate.load(), st.last_channels.load(), st.non_float_streams.load(),
        m_revision, hasConfig ? "true" : "false", Narrow(m_lastError).c_str(), m_statusWriteError);
    if (n <= 0) return;

    auto writeAtomically = [&](const std::wstring& path) -> DWORD {
        std::wstring tmp = path + L".tmp";
        HANDLE h = CreateFileW(tmp.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
        if (h == INVALID_HANDLE_VALUE) return GetLastError();
        DWORD wrote = 0;
        WriteFile(h, json, static_cast<DWORD>(n), &wrote, nullptr);
        CloseHandle(h);
        return MoveFileExW(tmp.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING) ? ERROR_SUCCESS : GetLastError();
    };
    m_statusWriteError = writeAtomically(m_statusPath);
    if (m_statusWriteError != ERROR_SUCCESS) {
        // Fallback so a permission problem never hides the fact that the APO is running:
        // the audio service's own temp folder (C:\Windows\ServiceProfiles\LocalService\AppData\Local\Temp).
        wchar_t tmpDir[MAX_PATH];
        DWORD len = GetTempPathW(MAX_PATH, tmpDir);
        if (len > 0 && len < MAX_PATH) writeAtomically(std::wstring(tmpDir) + L"diviora_apo_status.json");
    }
}
