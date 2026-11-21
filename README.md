# Diviora Psychoacoustics

A music player and **system-wide audio engine for Windows**. The developer's Equalizer
APO–style configuration and frequency watermark apply to all audio on the PC (the player,
browsers, Spotify, games), the way Equalizer APO 1.4.2 works, from a single installer that
needs no virtual cable and no Secure Boot changes. Configurations stay encrypted and
locked on users' PCs. The developer publishes updates once, and every activated device
applies them automatically. The app also updates itself.

| | |
|---|---|
| **Users** | [User Manual](docs/USER_MANUAL.md) · [Acceptance tests (how to check everything)](docs/ACCEPTANCE_TESTS.md) |
| **Developer** | [Developer Guide](docs/DEVELOPER_GUIDE.md) · [Native APO](windows-apo/README.md) · [Licence server](backend/README.md) · [DSP core](diviora-dsp/README.md) |
| **Legal** | [Licence](LICENSE.txt) · [Third-party notices](THIRD_PARTY_NOTICES.md) · [Security](SECURITY.md) |
| **Changes** | [Changelog](CHANGELOG.md) |

## Status

| Part | State |
|---|---|
| Native APO in `audiodg.exe` on composite (Realtek/Waves DCH) drivers | Working: verified on hardware, including a locked config changing YouTube audio live |
| Encrypted, ACL-locked configuration with live reload | Working (verified on hardware) |
| Frequency watermark (tone or uploaded clip) + detection | Implemented and tested in CI |
| Developer panel, password + recovery key, analysis, reports, export | Implemented |
| Licence server (one ID = one device, invites, config publishing) | Implemented and tested; deployment by the developer |
| Sync service (automatic configuration updates) | Implemented; to be verified on hardware |
| Single Windows installer + signed auto-update | Implemented; first signed release pending the developer's signing key |
| Android | Music player: library, in-app EQ + watermark, licence + config sync (APK built in CI; to be verified on a phone). Android does not allow processing other apps' audio |

## Build

```bat
windows-apo\build_native_apo.bat
npm install
npx tauri build
```

Every push is built and tested on Windows by GitHub Actions (`.github/workflows/windows-ci.yml`).
