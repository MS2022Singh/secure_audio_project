# Test signing key (test builds only)

`diviora-test.jks` (password `diviora-test`, alias `test`) signs the Android **test** APKs
that CI publishes as the `test-build` release. It is public on purpose: a fixed key lets each
new test build install over the previous one without uninstalling.

Never use it for a release to users. Real releases are signed with the developer's private
release key (see docs/DEVELOPER_GUIDE.md, "Before giving the app to users").
