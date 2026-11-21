//! diviora-sync: background Windows service (runs as LocalSystem) that applies the
//! developer's latest configuration to this PC.
//!
//!   diviora-sync.exe --once      one sync pass in the console (diagnostics)
//!   diviora-sync.exe --service   entry point used by the Service Control Manager
//!
//! The installer registers it:  sc.exe create DivioraSync binPath= "...\diviora-sync.exe --service" start= auto
use std::time::Duration;

const SERVICE_NAME: &str = "DivioraSync";
const INTERVAL: Duration = Duration::from_secs(5 * 60);

fn log(msg: &str) {
    let line = format!("{} {}\n", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0), msg);
    let path = diviora_client::state::client_dir().join("sync.log");
    let _ = std::fs::create_dir_all(diviora_client::state::client_dir());
    // Keep the log small: start over once it passes 256 KB.
    if std::fs::metadata(&path).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn pass() {
    match diviora_client::sync::sync_once() {
        Ok(o) => log(&format!("sync: {:?}", o)),
        Err(e) => log(&format!("sync error: {}", e)),
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--once") => match diviora_client::sync::sync_once() {
            Ok(o) => println!("{:?}", o),
            Err(e) => { eprintln!("{}", e); std::process::exit(1) }
        },
        Some("--service") => service::run(),
        _ => eprintln!("usage: diviora-sync.exe --once | --service"),
    }
}

#[cfg(windows)]
mod service {
    use super::*;
    use std::sync::mpsc;
    use windows_service::service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType};
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::{define_windows_service, service_dispatcher};

    define_windows_service!(ffi_service_main, service_main);

    pub fn run() {
        if let Err(e) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
            eprintln!("Not started by the Service Control Manager: {}", e);
        }
    }

    fn service_main(_args: Vec<std::ffi::OsString>) {
        let (stop_tx, stop_rx) = mpsc::channel();
        let handler = move |c| match c {
            ServiceControl::Stop | ServiceControl::Shutdown => { let _ = stop_tx.send(()); ServiceControlHandlerResult::NoError }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        };
        let Ok(status) = service_control_handler::register(SERVICE_NAME, handler) else { return };
        let set = |state, accept| {
            let _ = status.set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS, current_state: state, controls_accepted: accept,
                exit_code: ServiceExitCode::Win32(0), checkpoint: 0, wait_hint: Duration::from_secs(5), process_id: None,
            });
        };
        set(ServiceState::Running, ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN);
        log("service started");
        loop {
            pass();
            if stop_rx.recv_timeout(INTERVAL).is_ok() { break; }
        }
        log("service stopped");
        set(ServiceState::Stopped, ServiceControlAccept::empty());
    }
}

#[cfg(not(windows))]
mod service {
    pub fn run() {
        // Non-Windows (development): run passes in the foreground.
        loop { super::pass(); std::thread::sleep(super::INTERVAL); }
    }
}
