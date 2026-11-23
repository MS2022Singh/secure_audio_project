# Test signing key (test builds only)

`diviora-test.jks` (password `diviora-test`, alias `test`) signs the Android **test** APKs
that CI publishes as the `test-build` release. It is public on purpose: a fixed key lets each
new test build install over the previous one without uninstalling.

Never use it for a release to users. Real releases are signed with the developer's private
release key (see docs/DEVELOPER_GUIDE.md, "Before giving the app to users").

`updater-test.key` / `.pub` (password `diviora-test`) sign the Windows **test** builds' automatic
updates (`latest.json` in the `test-build` release). Also public on purpose and test-only: real
releases are built by `release.yml` with your private updater key from GitHub secrets
(`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_UPDATER_PUBKEY`).
