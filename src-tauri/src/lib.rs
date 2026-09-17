// LifeOS Core (Rust). M1 scope: visual shell only. M2 (this run) adds
// the first real domain vertical slice: Goal, through
// domain -> actions -> services -> private services::storage ->
// encrypted SQLCipher (LIFEOS_SPEC.md §5.4-§5.6). Real Tauri commands
// (a `capability` module) are not added yet - nothing here is wired
// into `run()` this run ("No React wiring yet. No UI changes yet.").
//
// None of these modules are `pub` from the crate root - nothing needs
// to call them from outside this crate yet (there is no Tauri command
// layer or NeXa transport using them), and they must not be reachable
// as a shortcut around the eventual capability boundary.
mod actions;
mod core_error;
mod domain;
mod security;
mod services;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
