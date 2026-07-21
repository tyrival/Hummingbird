use tauri::{Emitter, Manager};

pub mod ai;
pub mod analyse_commands;
pub mod analyse_task;
pub mod chunking;
pub mod commands;
pub mod crypto;
pub mod error;
pub mod extraction;
pub mod log_parse;
pub mod log_stats;
pub mod naming;
pub mod output;
pub mod prompt;
pub mod register_csv;
pub mod settings;
pub mod sftp_download;
pub mod task;
pub mod updater;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let settings = settings::SettingsStore::load_or_migrate(app.handle());
            let staging_root = app.path().app_data_dir()?.join("input-staging");
            app.manage(commands::CommandState::production(settings, staging_root));
            app.manage(updater::SignedUpdateState::default());
            app.manage(analyse_commands::AnalyseAppState {
                task_manager: analyse_task::AnalyseTaskManager::new(),
            });
            if let Some(window) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop {
                        paths, ..
                    }) = event
                    {
                        let generation = app_handle
                            .state::<commands::CommandState>()
                            .next_drop_generation();
                        let dropped_paths = paths.clone();
                        let event_app = app_handle.clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            let state = event_app.state::<commands::CommandState>();
                            let payload = match state
                                .authorize_os_dropped_paths_if_current(&dropped_paths, generation)
                            {
                                Some(Ok(input)) => {
                                    Some(commands::InputDropResult::Success { input })
                                }
                                Some(Err(error)) => {
                                    Some(commands::InputDropResult::Error { error })
                                }
                                None => None,
                            };
                            if let Some(payload) = payload {
                                let _ = event_app.emit("input-drop-result", payload);
                            }
                        });
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::select_input_file,
            commands::select_output_directory,
            commands::start_extraction,
            commands::cancel_extraction,
            commands::cancel_extraction_and_wait,
            commands::prepare_exit,
            commands::get_task_status,
            commands::open_output_directory,
            commands::open_task_output_directory,
            commands::get_app_version,
            updater::check_for_update,
            updater::download_update,
            updater::install_downloaded_update,
            updater::relaunch_app,
            analyse_commands::get_analyse_config,
            analyse_commands::save_ssh_servers,
            analyse_commands::test_ssh_connection,
            analyse_commands::list_remote_logs_command,
            analyse_commands::download_logs_command,
            analyse_commands::start_log_analysis,
            analyse_commands::cancel_log_analysis,
            analyse_commands::get_analyse_status,
            analyse_commands::select_log_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hummingbird application");
}
