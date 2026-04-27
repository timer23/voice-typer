use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{WavSpec, WavWriter};

struct SendableStream(cpal::Stream);
unsafe impl Send for SendableStream {}
unsafe impl Sync for SendableStream {}

pub struct AudioRecorder {
    host: cpal::Host,
    stream: Option<SendableStream>,
    recording: Arc<AtomicBool>,
    output_path: Option<PathBuf>,
    writer: Option<Arc<std::sync::Mutex<Option<WavWriter<BufWriter<File>>>>>>,
}

const VAD_THRESHOLD: f32 = 0.01; // RMS ~-40dB

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl AudioRecorder {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            host: cpal::default_host(),
            stream: None,
            recording: Arc::new(AtomicBool::new(false)),
            output_path: None,
            writer: None,
        })
    }

    pub fn start_recording(
        &mut self,
        last_speech_ms: Arc<AtomicU64>,
        speech_detected: Arc<AtomicBool>,
    ) -> anyhow::Result<()> {
        let device = self.host.default_input_device()
            .ok_or_else(|| anyhow::anyhow!("Микрофон не найден"))?;

        let config: cpal::StreamConfig = device.default_input_config()?.into();
        let output_path = std::env::temp_dir().join("voice_typer_recording.wav");
        self.output_path = Some(output_path.clone());

        let spec = WavSpec {
            channels: config.channels as u16,
            sample_rate: config.sample_rate.0,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let writer_arc = Arc::new(std::sync::Mutex::new(
            Some(WavWriter::create(&output_path, spec)?)
        ));
        let writer_clone = writer_arc.clone();
        self.writer = Some(writer_arc);

        self.recording.store(true, Ordering::SeqCst);
        let recording = self.recording.clone();

        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if !recording.load(Ordering::SeqCst) { return; }

                // VAD: считаем RMS и обновляем время последней речи
                let rms = (data.iter().map(|&s| s * s).sum::<f32>() / data.len() as f32).sqrt();
                if rms > VAD_THRESHOLD {
                    last_speech_ms.store(now_ms(), Ordering::Relaxed);
                    speech_detected.store(true, Ordering::Relaxed);
                }

                if let Ok(mut guard) = writer_clone.lock() {
                    if let Some(ref mut w) = *guard {
                        for &sample in data {
                            let s = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                            w.write_sample(s).ok();
                        }
                    }
                }
            },
            |err| eprintln!("Ошибка записи: {}", err),
            None,
        )?;

        stream.play()?;
        self.stream = Some(SendableStream(stream));
        Ok(())
    }

    pub fn stop_recording(&mut self) -> anyhow::Result<PathBuf> {
        self.recording.store(false, Ordering::SeqCst);

        if let Some(s) = self.stream.take() {
            s.0.pause().ok();
            drop(s);
        }

        std::thread::sleep(std::time::Duration::from_millis(50));

        if let Some(writer_arc) = self.writer.take() {
            let mut guard = writer_arc.lock()
                .map_err(|_| anyhow::anyhow!("Не удалось заблокировать writer"))?;
            if let Some(w) = guard.take() {
                w.finalize()?;
            }
        }

        self.output_path.take()
            .ok_or_else(|| anyhow::anyhow!("Запись не была начата"))
    }
}
