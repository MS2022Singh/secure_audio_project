// Desktop launcher; everything lives in lib.rs (shared with Android).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = app_lib::handle_cli() {
        std::process::exit(code);
    }
    app_lib::run()
}
