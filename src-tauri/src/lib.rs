// LifeOS Core (Rust). M1 scope: visual shell only - no domain commands,
// no database, no AI. Real Tauri commands arrive with M2 (LifeOS Core +
// encrypted storage) via the guarded action layer (LIFEOS_SPEC.md §2).
//
// `services` currently holds only the M2 encrypted-storage spike
// (2026-09-17) - a proof of the storage architecture, not real domain
// commands. Not wired into `run()` yet; exercised by its own tests.
// Not `pub` from the crate root - nothing needs to call it from outside
// this crate yet (there is no `capability` layer or Tauri command using
// it), and it must not be reachable as a shortcut around the eventual
// capability boundary.
mod services;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
