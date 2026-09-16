// LifeOS Core (Rust). M1 scope: visual shell only - no domain commands,
// no database, no AI. Real Tauri commands arrive with M2 (LifeOS Core +
// encrypted storage) via the guarded action layer (LIFEOS_SPEC.md §2).

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
