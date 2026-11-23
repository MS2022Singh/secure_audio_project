#!/usr/bin/env bash
# Install the APK on the emulator, launch Diviora, and fail if it crashes.
set -u
APK="$1"
PKG=com.diviora.psychoacoustics
adb install -r "$APK" || exit 1
adb logcat -c
adb shell monkey -p "$PKG" -c android.intent.category.LAUNCHER 1
sleep 25
adb logcat -d > logcat.txt
echo "=== Diviora / crash lines ==="
grep -E "FATAL EXCEPTION|AndroidRuntime|RustStdoutStderr|panicked|Tauri|$PKG|chromium" logcat.txt | tail -80
if grep -q "FATAL EXCEPTION" logcat.txt || grep -q "panicked at" logcat.txt; then
  echo "::error::Diviora crashed on start (see logcat.txt artifact)"
  exit 1
fi
if ! adb shell pidof "$PKG" >/dev/null; then
  echo "::error::Diviora is not running 25 s after launch"
  exit 1
fi
echo "Diviora started and is still running."
