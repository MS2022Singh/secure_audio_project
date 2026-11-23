# Project memory: DIVIORA PSYCHOACOUSTICS

## Goal
An installable app (Windows now, Android later). The developer uploads an Equalizer
APO–style configuration and sets a frequency watermark in a hidden, password-protected
developer panel. That configuration modifies **all system audio** on users' devices, the way
Equalizer APO 1.4.2 does, and every processed file or stream carries the watermark. Users can't
see or copy the configuration. One licence ID = one device. Developer updates and app updates
reach users automatically. It ships as one installer package.

## Decisions (keep)
- **No VB-Cable or virtual device.** Windows uses a native APO inside `audiodg.exe` on the real speakers.
- No BIOS or Secure Boot changes. Never replace registry permissions wholesale; add an entry
  temporarily and restore the exact original permissions.
- Configuration on disk only as an **encrypted (DPAPI), ACL-locked** `config.dvr`. Upload is held
  in memory until Lock Config.
- Don't change the UI unless asked. The watermark card (Signature ID, carrier, amplitude,
  upload audio) was requested.

## Hard-won facts (Windows, target laptop: Realtek DCH + Waves)
1. The driver uses **composite** effect lists (`{d04e05a6-…},13/14/15`). The legacy `,5/,6`
   slots are ignored. Diviora is prepended to the `,13` stream-effect list; Waves is kept.
2. `audiodg.exe` creates APOs **COM-aggregated** (`CreateInstance(outer, IID_IUnknown)`). An APO
   that refuses aggregation loads but is never instantiated. This blocked every attempt before
   2026-11-21.
3. `audiodg` runs with a **write-restricted** token: writable folders need `NT SERVICE\Audiosrv`
   or `WRITE RESTRICTED` in their ACL.
4. FxProperties keys aren't writable by Administrators directly (see the injector's temporary-ACE method).
5. The old `windows-service` VB-Cable bridge wrote fake "APO" diagnostics. It and the other legacy
   scripts were removed; they remain in git history (last present in commit `86f1002`).

## Status (2026-11-21)
- [x] Native APO loaded, instantiated and processing in `audiodg.exe`. A locked `-24 dB` test config
      changed YouTube volume live (verified on hardware).
- [x] Watermark engine + detection; developer panel features real (export, reports, analysis).
- [x] Password backdoors removed; PBKDF2 + recovery key; MachineGuid device ID.
- [x] Licence/config server, shared client, `diviora-sync` service (server tested; service and
      server deployment still to be verified on hardware).
- [x] NSIS installer + signed auto-update + CI/release workflows (first signed release needs the
      developer's signing key and GitHub secrets).
- [ ] Real configuration locked by the developer (deferred by the developer).
- [ ] Android: scope decision pending (Android allows full processing only in the app's own
      player; for other apps only per-session EQ, and no watermark).

## Where things are
See README.md, docs/DEVELOPER_GUIDE.md, windows-apo/README.md.
