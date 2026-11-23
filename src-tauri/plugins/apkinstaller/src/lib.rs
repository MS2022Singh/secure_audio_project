//! Android: open the system installer for an APK the app downloaded into its own cache
//! ("Do you want to update this app?"). Nothing goes to the Downloads folder.
use tauri::{plugin::{Builder, TauriPlugin}, Runtime};
#[cfg(target_os = "android")]
use tauri::Manager;

#[cfg(target_os = "android")]
pub struct ApkInstaller<R: Runtime>(tauri::plugin::PluginHandle<R>);

#[cfg(target_os = "android")]
#[derive(serde::Serialize)]
struct InstallArgs {
    path: String,
}

#[cfg(target_os = "android")]
impl<R: Runtime> ApkInstaller<R> {
    /// Ok(true) = installer opened; Ok(false) = Android first asks to allow installs from
    /// this app (the settings page was opened; try again after allowing).
    pub fn install(&self, path: String) -> Result<bool, String> {
        let r: serde_json::Value = self.0.run_mobile_plugin("installApk", InstallArgs { path }).map_err(|e| e.to_string())?;
        Ok(!r.get("needsPermission").and_then(|v| v.as_bool()).unwrap_or(false))
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("apkinstaller")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                let handle = _api.register_android_plugin("com.diviora.apkinstaller", "ApkInstallerPlugin")?;
                _app.manage(ApkInstaller(handle));
            }
            Ok(())
        })
        .build()
}
