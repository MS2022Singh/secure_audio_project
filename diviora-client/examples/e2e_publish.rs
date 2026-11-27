//! Local end-to-end check against backend/server.py on 127.0.0.1:
//! e2e_publish <server_url> <admin_token> <licence_key> <work_dir>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (url, admin, key, dir) = (&a[1], &a[2], &a[3], std::path::PathBuf::from(&a[4]));
    diviora_client::state::set_base_dir(dir.join("device"));
    std::env::set_var("LOCALAPPDATA", dir.join("dev"));
    let s = diviora_client::server::activate(url, key, "DEVICE-E2E-0001").expect("activate");
    let mut st = diviora_client::state::ClientState::default();
    st.server_url = url.clone(); st.license_key = s.license_key; st.device_id = "DEVICE-E2E-0001".into();
    st.session = s.session; st.session_expires_at = s.session_expires_at; st.licence_expires_at = s.licence_expires_at;
    st.save().unwrap();
    diviora_client::state::save_developer_credentials(url, admin).unwrap();
    let dev = diviora_client::state::developer_credentials().unwrap();
    let (v, n, old) = diviora_client::server::admin_publish_config(&dev.server_url, &dev.admin_token, "Preamp: -3 dB\nFilter: ON PK Fc 1000 Hz Gain 2 dB Q 1\n").unwrap();
    println!("published version {} for {} device(s), {} old app(s)", v, n, old);
    let got = diviora_client::sync::fetch_latest_config().unwrap().unwrap();
    println!("device decrypted version {}: {:?}", got.0, got.1);
    assert!(got.1.starts_with("Preamp: -3 dB"));
}
