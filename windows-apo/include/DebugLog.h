// Diagnostic messages, readable live with Sysinternals DebugView
// (run as Administrator, Capture > "Capture Global Win32"). Works inside audiodg.exe
// where file writes can be blocked. Never called per audio buffer (only on the first one).
#pragma once
#include <windows.h>
#include <cstdarg>
#include <cstdio>
#include <objbase.h>

inline void DvrLog(const wchar_t* fmt, ...) {
    wchar_t msg[512];
    int n = _snwprintf_s(msg, _countof(msg), _TRUNCATE, L"[Diviora] pid=%lu tid=%lu ",
                         GetCurrentProcessId(), GetCurrentThreadId());
    if (n < 0) n = 0;
    va_list ap;
    va_start(ap, fmt);
    _vsnwprintf_s(msg + n, _countof(msg) - n, _TRUNCATE, fmt, ap);
    va_end(ap);
    OutputDebugStringW(msg);
}

// "{xxxxxxxx-...}" for logging interface/class IDs.
struct DvrGuidStr {
    wchar_t s[40];
    explicit DvrGuidStr(REFGUID g) { if (!StringFromGUID2(g, s, 40)) s[0] = 0; }
};
