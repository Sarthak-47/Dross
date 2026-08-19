//! Tauri backend — thin IPC layer over dross-core. All analysis logic lives
//! in the core crate so the app and the CLI cannot drift apart.

mod commands;
mod state;
mod watcher;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_repository,
            commands::current_repository,
            commands::analyze,
            commands::build_index,
            commands::index_status,
            commands::list_connections,
            commands::install_connection,
            commands::uninstall_connection,
            commands::risk_history,
            commands::get_config,
            commands::set_config,
            commands::override_authorship,
            commands::file_source,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
