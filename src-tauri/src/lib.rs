use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use tokio::sync::Mutex;

mod audio;
mod whisper;
mod input;

pub use audio::AudioRecorder;
pub use whisper::WhisperEngine;
pub use input::InputInjector;

pub struct AppState {
    pub recording: Arc<AtomicBool>,
    pub recorder: Arc<Mutex<AudioRecorder>>,
    pub whisper: Arc<Mutex<WhisperEngine>>,
    pub input: Arc<Mutex<InputInjector>>,
    // VAD: отслеживание голосовой активности
    pub last_speech_ms: Arc<AtomicU64>,
    pub speech_detected: Arc<AtomicBool>,
    pub hotkey_mode: Arc<AtomicBool>,
    // Последнее активное окно НЕ из нашего процесса (обновляется фоновым потоком)
    pub last_target_hwnd: Arc<AtomicUsize>,
}

impl AppState {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            recording: Arc::new(AtomicBool::new(false)),
            recorder: Arc::new(Mutex::new(AudioRecorder::new()?)),
            whisper: Arc::new(Mutex::new(WhisperEngine::new()?)),
            input: Arc::new(Mutex::new(InputInjector::new())),
            last_speech_ms: Arc::new(AtomicU64::new(0)),
            speech_detected: Arc::new(AtomicBool::new(false)),
            hotkey_mode: Arc::new(AtomicBool::new(false)),
            last_target_hwnd: Arc::new(AtomicUsize::new(0)),
        })
    }
}
