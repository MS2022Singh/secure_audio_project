fn main() {
    // Android: oboe (used by cpal/rodio for audio output) is C++ and needs the NDK C++ runtime.
    // Link libc++_shared; CI copies libc++_shared.so into the APK's jniLibs.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=c++_shared");
    }
    tauri_build::build();
}
