# Changelog

## 0.2.0 (unreleased)

### System-wide audio (Windows)
- New native APO, loaded by Windows audio (`audiodg.exe`) on the speakers' composite
  stream-effect list, the way Equalizer APO works. It supports COM aggregation, which Windows
  audio requires; earlier builds refused it and were never instantiated.
- The APO reads the encrypted, locked configuration itself and reloads it live. Errors fall
  back to pass-through.
- Install, verify and uninstall with an exact backup and restore of speaker settings and registry
  permissions. Diagnostics: `-Diagnose`, DebugView logging, canary build.
- Retired: the VB-Cable bridge service and the registry hacks that replaced driver permissions.

### Watermark
- Frequency-signature watermark keyed with the Signature ID, or an uploaded audio clip.
  Time window, interval, auto mode (only under music), and detection in files.

### App
- Removed the password backdoors. First-run password setup, recovery key and salted PBKDF2.
- Download Processed Audio renders EQ + watermark to WAV. Reports no longer leak the configuration.
- Measured analysis values (peak gain, latency, init time, CPU).
- Licence activation, invite tokens and configuration publishing through the licence server.

### Distribution
- Licence and configuration server (one ID = one device, invites, revocation, versioned configs).
- Background sync service applies the developer's updates automatically.
- Single NSIS installer (app + APO + service) and signed automatic app updates.
- Windows CI on every push; release workflow on version tags.
- Documentation: user manual, developer guide, licence, third-party notices, security policy.
