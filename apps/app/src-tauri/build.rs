//! Build-Skript der Tauri-Shell: Kontext, Icons und die Berechtigungen der eigenen Commands.
//!
//! Die eigenen Commands werden im App-Manifest aufgeführt, damit sie wie Plugin-Commands nur über
//! `capabilities/default.json` freigeschaltet sind (`allow-core-view`, `allow-core-dispatch`, …).

fn main() {
    let attributes = tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
        "core_view",
        "core_dispatch",
        "app_info",
        "open_log_dir",
        "reveal_output",
        "license_text",
        "mobile_pick_files",
        "demo_document",
    ]));
    tauri_build::try_build(attributes).expect("tauri-build fehlgeschlagen");
}
