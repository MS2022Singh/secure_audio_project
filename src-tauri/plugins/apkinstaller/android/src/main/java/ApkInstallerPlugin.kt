package com.diviora.apkinstaller

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File

/** Own subclass so it never clashes with another FileProvider in the app. */
class ApkFileProvider : FileProvider()

@InvokeArg
class InstallArgs {
    lateinit var path: String
}

@TauriPlugin
class ApkInstallerPlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun installApk(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(InstallArgs::class.java)
            val result = JSObject()
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
                !activity.packageManager.canRequestPackageInstalls()) {
                // First time only: Android asks the user to allow installs from Diviora.
                val settings = Intent(
                    Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                    Uri.parse("package:" + activity.packageName)
                )
                settings.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                activity.startActivity(settings)
                result.put("needsPermission", true)
                invoke.resolve(result)
                return
            }
            val file = File(args.path)
            val uri = FileProvider.getUriForFile(
                activity, activity.packageName + ".apkinstaller.fileprovider", file
            )
            val intent = Intent(Intent.ACTION_VIEW)
            intent.setDataAndType(uri, "application/vnd.android.package-archive")
            intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            activity.startActivity(intent)
            result.put("needsPermission", false)
            invoke.resolve(result)
        } catch (ex: Exception) {
            invoke.reject(ex.message ?: "Could not open the installer")
        }
    }
}
