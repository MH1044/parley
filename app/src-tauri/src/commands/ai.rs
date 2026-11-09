//! The AI commands (docs/modules/06-publik-and-providers.md, Commands and events): provider choice,
//! consent, the own key, the local model, the connection check, publik API's links, and the status.
//!
//! Each command marshals into `parley_publik::service::AiService` and nothing more. The work runs
//! on a blocking thread, never on the window's main thread or the async runtime, because most of it
//! reaches the network. Every command that changes the status emits it as `ai://status`. Nothing
//! returned or emitted holds a key: the interface receives `AiStatus`, which has no key field
//! (invariant I1).

use parley_core::ai::{AiStatus, ConnectionCheck, LocalModel};
use parley_core::api::AI_STATUS_EVENT;
use parley_core::error::{AppError, ErrorCode};
use parley_core::settings::AiSettings;
use parley_publik::service::{AiService, OwnKeyInput};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// The error of an AI command whose work stopped before it finished.
pub fn unfinished() -> AppError {
    AppError::new(
        ErrorCode::ProviderError,
        "Parley could not finish that AI request.",
    )
}

/// The error of a link the browser did not open.
pub fn link_not_opened() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Parley could not open the link in your browser.",
    )
}

/// Runs `f` on a blocking thread with the AI service and the AI settings; saves the settings when
/// `f` changed them, and emits the status when `emit` is set.
///
/// All of it happens while the AI service is locked, and the settings are read after the lock is
/// taken, so two overlapping AI commands cannot both start from the same settings and lose one
/// change. The settings lock is taken only briefly inside it (to read and to save), always in that
/// order, AI service first; no command takes them the other way round.
async fn with_ai<T: Send + 'static>(
    app: AppHandle,
    emit: bool,
    f: impl FnOnce(&mut AiService, &mut AiSettings) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with_ai(|service| {
            let before = state.with(|p| p.settings_get().ai);
            let mut ai = before.clone();
            let result = f(service, &mut ai);
            if ai != before {
                state.with(|p| p.save_ai_settings(&ai))?;
            }
            if emit {
                let status = service.status(&ai, false);
                // A window that cannot hear the event still gets the result.
                let _ = app.emit(AI_STATUS_EVENT, &status);
            }
            result
        })
    })
    .await
    .map_err(|_| unfinished())?
}

/// Start-up (`app_start`): the service's start, on a thread of its own so the first screen never
/// waits for the network; the status arrives as `ai://status`.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let status = state.with_ai(|service| {
            let ai = state.with(|p| p.settings_get().ai);
            service.on_start(&ai)
        });
        let _ = app.emit(AI_STATUS_EVENT, &status);
    });
}

/// "Delete everything" (`data_delete_all`): forgets the AI status once the data is gone. Called
/// after the settings lock is released, never inside it.
pub fn forget(state: &AppState) {
    state.with_ai(|service| service.forget_status());
}

/// The status; with `refresh` (Settings > AI opening), one wallet refresh while publik API is the
/// chosen provider. The status is also emitted, so every view showing it stays current.
#[tauri::command(rename_all = "snake_case")]
pub async fn ai_status(app: AppHandle, refresh: bool) -> Result<AiStatus, AppError> {
    with_ai(
        app,
        true,
        move |service, ai| Ok(service.status(ai, refresh)),
    )
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ai_consent_accept(app: AppHandle) -> Result<AiStatus, AppError> {
    with_ai(app, true, |service, ai| service.consent_accept(ai)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ai_consent_decline(app: AppHandle) -> Result<AiStatus, AppError> {
    with_ai(app, false, |service, ai| Ok(service.consent_decline(ai))).await
}

#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
pub async fn ai_set_own_key(
    app: AppHandle,
    base_url: String,
    key: String,
    chat_model: String,
    transcribe_model: Option<String>,
    speech_model: Option<String>,
    speech_voice: Option<String>,
) -> Result<AiStatus, AppError> {
    let input = OwnKeyInput {
        base_url,
        key,
        chat_model,
        transcribe_model,
        speech_model,
        speech_voice,
    };
    with_ai(app, true, move |service, ai| {
        service.set_own_key(ai, &input)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ai_clear_own_key(app: AppHandle) -> Result<AiStatus, AppError> {
    with_ai(app, true, |service, ai| service.clear_own_key(ai)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ai_local_probe(app: AppHandle, base_url: String) -> Result<Vec<LocalModel>, AppError> {
    with_ai(app, true, move |service, _| service.local_probe(&base_url)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ai_set_local(
    app: AppHandle,
    base_url: String,
    chat_model: Option<String>,
) -> Result<AiStatus, AppError> {
    with_ai(app, true, move |service, ai| {
        service.set_local(ai, &base_url, chat_model.as_deref())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ai_check_connection(app: AppHandle) -> Result<ConnectionCheck, AppError> {
    with_ai(app, true, |service, ai| service.check_connection(ai)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ai_disconnect_publik(app: AppHandle) -> Result<AiStatus, AppError> {
    with_ai(app, true, |service, ai| Ok(service.disconnect_publik(ai))).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ai_reconnect_publik(app: AppHandle) -> Result<AiStatus, AppError> {
    with_ai(app, true, |service, ai| service.reconnect_publik(ai)).await
}

/// Opens the claim, top-up or add-credit link of the status in the browser. It takes only the
/// kind: the URL is the core's own copy, never one from the interface. Opened from Rust, so the
/// web view needs no opener permission.
#[tauri::command(rename_all = "snake_case")]
pub async fn ai_open_link(app: AppHandle, kind: String) -> Result<(), AppError> {
    let url = with_ai(app, false, move |service, ai| {
        service.status(ai, false);
        service.link_url(&kind)
    })
    .await?;
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "This link is not available yet.",
        ));
    }
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|_| link_not_opened())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AI_BACKOFF;
    use std::time::Duration;

    #[test]
    fn the_desktop_only_messages() {
        let unfinished = unfinished();
        assert_eq!(
            (
                unfinished.code,
                unfinished.message.as_str(),
                unfinished.action
            ),
            (
                ErrorCode::ProviderError,
                "Parley could not finish that AI request.",
                None
            )
        );
        let not_opened = link_not_opened();
        assert_eq!(
            (
                not_opened.code,
                not_opened.message.as_str(),
                not_opened.action
            ),
            (
                ErrorCode::NotFound,
                "Parley could not open the link in your browser.",
                None
            )
        );
    }

    #[test]
    fn own_key_and_local_calls_wait_one_then_four_seconds_between_tries() {
        assert_eq!(AI_BACKOFF, [Duration::from_secs(1), Duration::from_secs(4)]);
    }
}
