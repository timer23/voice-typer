#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use voice_typer_lib::AppState;

#[cfg(target_os = "windows")]
extern "system" {
    fn GetForegroundWindow() -> isize;
    fn GetCurrentProcessId() -> u32;
    fn GetWindowThreadProcessId(hwnd: isize, lpdwProcessId: *mut u32) -> u32;
}

// ── Конфиг ──────────────────────────────────────────────────────────────────

fn config_path() -> std::path::PathBuf {
    std::env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("voice-typer")
        .join("config.json")
}

fn load_config() -> Option<String> {
    let text = std::fs::read_to_string(config_path()).ok()?;
    let val: serde_json::Value = serde_json::from_str(&text).ok()?;
    val["api_key"].as_str().map(|s| s.to_string())
}

fn save_config(key: &str) -> anyhow::Result<()> {
    let path = config_path();
    if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
    std::fs::write(path, serde_json::to_string_pretty(&serde_json::json!({ "api_key": key }))?)?;
    Ok(())
}

// ── Логика записи (вызывается и из команд, и из хоткея) ─────────────────────

async fn do_start(state: &AppState, app: tauri::AppHandle, from_hotkey: bool) -> Result<(), String> {
    if state.recording.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Err("Уже идёт запись".to_string());
    }

    state.hotkey_mode.store(from_hotkey, Ordering::Relaxed);
    state.speech_detected.store(false, Ordering::Relaxed);
    state.last_speech_ms.store(0, Ordering::Relaxed);

    {
        let mut input = state.input.lock().await;
        input.clear_saved_focus();
        if from_hotkey {
            input.save_focus(); // активное окно ДО хоткея
        } else {
            let hwnd = state.last_target_hwnd.load(Ordering::Relaxed) as isize;
            input.save_focus_hwnd(hwnd);
            // Сразу возвращаем фокус целевому окну пока наш процесс ещё foreground
            input.restore_focus_quick();
        }
    }

    {
        let mut recorder = state.recorder.lock().await;
        recorder.start_recording(state.last_speech_ms.clone(), state.speech_detected.clone())
            .map_err(|e| {
                state.recording.store(false, Ordering::SeqCst);
                format!("Ошибка микрофона: {}", e)
            })?;
    }

    app.emit("recording-state", true).ok();

    Ok(())
}

async fn do_stop(state: &AppState, app: &tauri::AppHandle) -> Result<String, String> {
    if state.recording.compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Err("Запись не активна".to_string());
    }

    let audio_path = {
        let mut rec = state.recorder.lock().await;
        rec.stop_recording().map_err(|e| format!("{}", e))?
    };

    app.emit("recording-state", false).ok();
    app.emit("status", "Распознаю...").ok();

    let text = {
        let w = state.whisper.lock().await;
        w.transcribe(&audio_path).await.map_err(|e| format!("{}", e))?
    };

    {
        let mut inp = state.input.lock().await;
        inp.type_text(&text).map_err(|e| format!("{}", e))?;
    }

    app.emit("transcription-ready", &text).ok();
    Ok(text)
}

// ── Tauri команды ────────────────────────────────────────────────────────────

#[tauri::command]
async fn start_recording(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<(), String> {
    do_start(&state, app, false).await
}

#[tauri::command]
async fn stop_and_transcribe(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<String, String> {
    do_stop(&state, &app).await
}

#[tauri::command]
fn is_recording(state: State<'_, AppState>) -> bool {
    state.recording.load(Ordering::SeqCst)
}

#[tauri::command]
async fn save_api_key(key: String, state: State<'_, AppState>) -> Result<(), String> {
    save_config(&key).map_err(|e| format!("{}", e))?;
    let mut whisper = state.whisper.lock().await;
    whisper.set_key(key);
    Ok(())
}

#[tauri::command]
fn open_devtools(app: tauri::AppHandle) {
    #[cfg(debug_assertions)]
    if let Some(w) = app.get_webview_window("main") {
        w.open_devtools();
    }
}

#[tauri::command]
async fn get_api_key(state: State<'_, AppState>) -> Result<String, String> {
    let whisper = state.whisper.lock().await;
    Ok(whisper.get_key().unwrap_or("").to_string())
}

// ── main ─────────────────────────────────────────────────────────────────────

fn main() {
    let app_state = {
        let state = AppState::new().expect("Не удалось инициализировать состояние");
        if let Some(key) = load_config() {
            if let Ok(mut whisper) = state.whisper.try_lock() {
                whisper.set_key(key);
            }
        }
        state
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(app_state)
        .setup(|app| {
            // Фоновый поток: запоминаем последнее активное окно не из нашего процесса
            {
                let last_hwnd = app.state::<AppState>().last_target_hwnd.clone();
                std::thread::spawn(move || {
                    #[cfg(target_os = "windows")]
                    let our_pid = unsafe { GetCurrentProcessId() };
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(250));
                        #[cfg(target_os = "windows")]
                        unsafe {
                            let hwnd = GetForegroundWindow();
                            let mut pid: u32 = 0;
                            GetWindowThreadProcessId(hwnd, &mut pid);
                            if pid != 0 && pid != our_pid {
                                last_hwnd.store(hwnd as usize, Ordering::Relaxed);
                            }
                        }
                    }
                });
            }

            // Регистрируем глобальный хоткей Ctrl+Shift+Space
            let shortcut = Shortcut::new(
                Some(Modifiers::CONTROL | Modifiers::SHIFT),
                Code::Space,
            );

            app.handle().global_shortcut().on_shortcut(shortcut, |app, _, event| {
                if event.state == ShortcutState::Pressed {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let state = app.state::<AppState>();
                        if state.recording.load(Ordering::SeqCst) {
                            do_stop(&state, &app).await.ok();
                        } else {
                            do_start(&state, app.clone(), true).await.ok();
                        }
                    });
                }
            })?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_and_transcribe,
            is_recording,
            save_api_key,
            get_api_key,
            open_devtools
        ])
        .run(tauri::generate_context!())
        .expect("Ошибка при запуске приложения");
}
