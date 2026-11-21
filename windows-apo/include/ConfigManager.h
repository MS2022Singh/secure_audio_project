// Process-wide (one per audiodg.exe) owner of the Diviora configuration.
//
// * Watches %ProgramData%\Diviora\apo\ for changes to config.dvr (like Equalizer APO
//   watches config.txt), decrypts it with DPAPI (machine scope, so a copied file is
//   useless on another PC) and hands the text to every live APO instance.
// * Writes %ProgramData%\Diviora\apo\status\apo_status.json once a second so the app
//   can prove the APO is running inside audiodg.exe and processing audio.
#pragma once
#include <windows.h>
#include <atomic>
#include <mutex>
#include <string>
#include <vector>

class DivioraAPO;

struct DivioraCounters {
    std::atomic<LONG>     instances{0};
    std::atomic<LONG>     locked{0};
    std::atomic<LONG64>   process_calls{0};
    std::atomic<LONG64>   processed_frames{0};
    std::atomic<LONG64>   filtered_frames{0};   // frames that went through a non-null engine
    std::atomic<LONG>     last_sample_rate{0};
    std::atomic<LONG>     last_channels{0};
    std::atomic<LONG>     non_float_streams{0};
};

DivioraCounters& DivioraStats();

class ConfigManager {
public:
    static ConfigManager& Instance();

    // Both called outside the audio thread.
    void Register(DivioraAPO* apo);
    void Unregister(DivioraAPO* apo);
    // LockForProcess/UnlockForProcess run under this lock so they never race a reload.
    std::mutex& Mutex() { return m_mutex; }
    // Current decrypted config text (caller holds Mutex()).
    const std::string& CurrentText() const { return m_text; }

private:
    ConfigManager() = default;
    void EnsureThread();
    void ThreadMain();
    bool ReloadIfChanged();       // returns true if text changed
    void WriteStatus();

    std::mutex m_mutex;
    std::vector<DivioraAPO*> m_instances;
    std::string m_text;
    std::atomic<bool> m_threadStarted{false};

    // Loader state (only touched by the watcher thread, except where noted).
    FILETIME m_lastWrite{};
    ULONGLONG m_lastSize = ~0ULL;
    LONG64 m_revision = 0;
    std::wstring m_lastError = L"none";
    DWORD m_statusWriteError = 0;   // last error writing the status file (0 = ok)
    std::wstring m_baseDir, m_configPath, m_statusDir, m_statusPath;
};
