const COMMANDS: &[&str] = &[
    "status",
    "setup",
    "unlock",
    "unlock_touch_id",
    "lock",
    "select_service",
    "set_overlay",
    "update_settings",
    "change_password",
    "set_touch_id",
    "sign_out",
    "erase_everything",
    "security_check",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
