#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod config;
mod logging;
mod monitors;
mod runtime;
mod state;

use state::AppState;

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::list_devices,
            commands::open_capture,
            commands::capture_info,
            commands::close_capture,
            commands::start_bar,
            commands::stop_bar,
            commands::stop_all,
            commands::status,
            commands::set_brightness,
            commands::set_all_color,
            commands::resume_sync,
            commands::cinema_start,
            commands::cinema_stop,
            commands::identify_bar,
            commands::get_config,
            commands::save_config,
            commands::autostart_enabled,
            commands::set_autostart,
            commands::autostart_run,
            commands::list_presets,
            commands::save_preset,
            commands::delete_preset
        ])
        .setup(|app| {
            runtime::seed_ident_cache(app.handle());
            runtime::spawn_autostart(app.handle().clone());
            runtime::spawn_hotplug(app.handle().clone());
            runtime::spawn_watchdog(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running bloqsync");
}