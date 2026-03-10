mod app_state;
mod commands;
mod runtime;
mod storage;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = runtime::startup_state()?;
            app.manage(state);

            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            app.handle().plugin(tauri_plugin_dialog::init())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::list_sessions,
            commands::get_session,
            commands::get_session_events,
            commands::pick_workspace,
            commands::start_run,
            commands::continue_session,
            commands::cancel_run,
            commands::retry_run,
            commands::delete_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}
