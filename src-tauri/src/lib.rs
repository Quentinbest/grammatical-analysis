pub mod commands;
pub mod domain;
pub mod llm;
pub mod os;
pub mod retrieval;
pub mod storage;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_sql::{Migration, MigrationKind};

use crate::commands::AppState;
use crate::os::clipboard::{SystemClipboard, DEFAULT_POLL_TIMEOUT_MS};
use crate::os::floating_window::{
    show_floating_window_near_cursor, Point, DEFAULT_WINDOW_MARGIN, SELECTION_PILL_WINDOW_LABEL,
};
use crate::os::hotkey::{
    build_selection_error_payload, emit_selection_error, trigger_hotkey_capture, HotkeySpec,
    DEFAULT_HOTKEY, SECONDARY_HOTKEY,
};
use crate::os::selection_monitor::{hide_pill_window, start_selection_monitor, SelectionAssistantState};
use crate::os::tray::{setup_tray, show_and_focus_main};
use crate::storage::{Storage, INIT_MIGRATION_SQL};

const MODIFIER_SETTLE_DELAY: Duration = Duration::from_millis(120);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let migrations = vec![Migration {
        version: 1,
        description: "init_grammatical_analysis_schema",
        sql: INIT_MIGRATION_SQL,
        kind: MigrationKind::Up,
    }];

    let assistant = Arc::new(SelectionAssistantState::new());
    let assistant_for_hotkey = Arc::clone(&assistant);
    let assistant_for_window = Arc::clone(&assistant);
    let assistant_for_setup = Arc::clone(&assistant);
    let assistant_for_run = Arc::clone(&assistant);

    tauri::Builder::default()
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:grammatical_analysis.db", migrations)
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if event.state() != ShortcutState::Released {
                        return;
                    }
                    if assistant_for_hotkey.clear_pending() {
                        hide_pill_window(app);
                    }
                    let app_handle = app.clone();
                    let shortcut_label = shortcut.to_string();
                    let initial_cursor = app_handle
                        .get_webview_window("main")
                        .and_then(|w| w.cursor_position().ok())
                        .map(|pos| Point {
                            x: pos.x.round() as i32,
                            y: pos.y.round() as i32,
                        })
                        .unwrap_or(Point { x: 240, y: 180 });

                    std::thread::spawn(move || {
                        let trigger_started = Instant::now();
                        std::thread::sleep(MODIFIER_SETTLE_DELAY);

                        let spec = HotkeySpec::parse(&shortcut_label).unwrap_or_default();
                        if let Err(err) = trigger_hotkey_capture(
                            &app_handle,
                            &SystemClipboard,
                            &spec,
                            initial_cursor,
                            DEFAULT_POLL_TIMEOUT_MS,
                        ) {
                            eprintln!("hotkey capture failed ({shortcut_label}): {err}");
                            let payload = build_selection_error_payload(&err, &spec.to_string());
                            if let Err(emit_err) = emit_selection_error(&app_handle, &payload) {
                                eprintln!("failed to emit selection://error: {emit_err}");
                            }
                        }

                        let main_app = app_handle.clone();
                        if let Err(err) = app_handle.run_on_main_thread(move || {
                            if let Some(win) = main_app.get_webview_window("main") {
                                let cursor = win
                                    .cursor_position()
                                    .ok()
                                    .map(|pos| Point {
                                        x: pos.x.round() as i32,
                                        y: pos.y.round() as i32,
                                    })
                                    .unwrap_or(initial_cursor);
                                if let Err(err) = show_floating_window_near_cursor(
                                    &win,
                                    cursor,
                                    DEFAULT_WINDOW_MARGIN,
                                    trigger_started,
                                ) {
                                    eprintln!("floating window placement failed: {err}");
                                }
                            }
                        }) {
                            eprintln!("failed to dispatch window show to main thread: {err}");
                        }
                    });
                })
                .build(),
        )
        .on_window_event(move |window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label();
                if (label == "main" || label == SELECTION_PILL_WINDOW_LABEL)
                    && !assistant_for_window.is_exiting()
                {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(move |app| {
            let db_path = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("grammatical-analysis"))
                .join("grammatical_analysis.db");
            let storage = Storage::open(&db_path)
                .map_err(|err| Box::new(err) as Box<dyn std::error::Error>)?;
            let resource_dir = app.path().resource_dir().ok();
            app.manage(AppState::with_resource_dir(Arc::new(storage), resource_dir));
            app.manage(Arc::clone(&assistant_for_setup));

            if let Err(err) = setup_tray(app.handle(), Arc::clone(&assistant_for_setup)) {
                eprintln!("tray icon setup failed: {err}");
            }
            start_selection_monitor(app.handle().clone(), Arc::clone(&assistant_for_setup));

            for shortcut in [DEFAULT_HOTKEY, SECONDARY_HOTKEY] {
                if let Err(err) = app.global_shortcut().register(shortcut) {
                    eprintln!("global shortcut {shortcut} registration failed: {err}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::run_analysis,
            commands::cancel_analysis,
            commands::list_providers,
            commands::upsert_provider,
            commands::set_api_key,
            commands::list_api_key_refs,
            commands::list_actions,
            commands::upsert_action,
            commands::save_history,
            commands::list_history,
            commands::read_clipboard_text,
            commands::get_selection_assistant_status,
            commands::set_selection_popup_enabled,
            commands::dismiss_selection_pill,
            commands::confirm_selection_pill,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |app_handle, event| match event {
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                if code.is_none() && !assistant_for_run.is_exiting() {
                    api.prevent_exit();
                }
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                show_and_focus_main(app_handle);
            }
            _ => {}
        });
}
