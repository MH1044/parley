//! Parley's desktop shell: a thin Tauri layer that owns the app state, turns commands into calls
//! into `parley-core`, and emits events. All learning logic lives in the core (docs/ARCHITECTURE.md
//! §3). The web view never holds a key and never touches the filesystem on its own; it only
//! sends intents and renders results, picks files through `file_pick`, which opens the native
//! dialog from Rust, and opens publik API's links through `ai_open_link`, which opens the core's own
//! copy of the link from Rust, so the window's capability stays `core:default`.

pub mod bundled;
pub mod commands;
pub mod env;
pub mod state;

use tauri::Manager;

/// Opens the window: the app state over the app data directory, the dialog plugin for
/// `file_pick`, and every command of `parley_core::api::COMMANDS`.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            // A directory that cannot be created shows the data-file screen at app_start.
            let _ = std::fs::create_dir_all(&data_dir);
            app.manage(state::AppState::new(data_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::app_start,
            commands::app::app_info,
            commands::app::file_pick,
            commands::app::data_delete_all,
            commands::app::cache_clear,
            commands::settings::settings_get,
            commands::settings::settings_set,
            commands::course::course_list,
            commands::course::course_start,
            commands::pack::pack_status,
            commands::pack::pack_download,
            commands::path::path_get,
            commands::path::skill_get,
            commands::placement::placement_start,
            commands::placement::placement_answer,
            commands::lesson::lesson_start,
            commands::lesson::review_start,
            commands::lesson::exercise_submit,
            commands::lesson::exercise_i_was_right,
            commands::lesson::lesson_finish,
            commands::stats::stats_get,
            commands::backup::backup_export,
            commands::backup::backup_import,
            commands::ai::ai_status,
            commands::ai::ai_consent_accept,
            commands::ai::ai_consent_decline,
            commands::ai::ai_set_own_key,
            commands::ai::ai_clear_own_key,
            commands::ai::ai_local_probe,
            commands::ai::ai_set_local,
            commands::ai::ai_check_connection,
            commands::ai::ai_disconnect_publik,
            commands::ai::ai_reconnect_publik,
            commands::ai::ai_open_link,
            commands::lesson::lesson_use_course_pack,
            commands::lesson::course_pack_export,
            commands::lesson::course_pack_import,
            commands::lesson::ai_lesson_checks,
            commands::lesson::exercise_check,
            commands::lesson::explain_answer,
        ])
        .run(tauri::generate_context!())
        .expect("Parley could not open its window");
}
