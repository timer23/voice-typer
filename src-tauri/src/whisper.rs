use std::path::Path;

pub struct WhisperEngine {
    api_key: Option<String>,
    api_url: String,
    model: String,
}

impl WhisperEngine {
    pub fn new() -> anyhow::Result<Self> {
        let mut engine = Self {
            api_key: None,
            api_url: String::new(),
            model: String::new(),
        };

        // Сначала пробуем переменные окружения
        if let Ok(key) = std::env::var("GROQ_API_KEY") {
            engine.apply_key(key);
        } else if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            engine.apply_key(key);
        }

        Ok(engine)
    }

    /// Установить ключ и автоматически определить провайдера
    pub fn set_key(&mut self, key: String) {
        self.apply_key(key);
    }

    pub fn get_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    fn apply_key(&mut self, key: String) {
        // Определяем провайдера по префиксу ключа
        if key.starts_with("gsk_") {
            self.api_url = "https://api.groq.com/openai/v1/audio/transcriptions".to_string();
            self.model = "whisper-large-v3".to_string();
        } else {
            self.api_url = "https://api.openai.com/v1/audio/transcriptions".to_string();
            self.model = "whisper-1".to_string();
        }
        self.api_key = Some(key);
    }

    pub async fn transcribe(&self, audio_path: &Path) -> anyhow::Result<String> {
        let api_key = self.api_key.as_ref()
            .ok_or_else(|| anyhow::anyhow!("API ключ не задан. Введите Groq или OpenAI ключ в настройках."))?;

        let file_content = tokio::fs::read(audio_path).await?;
        let filename = audio_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.wav")
            .to_string();

        let file_part = reqwest::multipart::Part::bytes(file_content)
            .file_name(filename)
            .mime_str("audio/wav")?;

        let form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("model", self.model.clone())
            .text("response_format", "json");

        let response = reqwest::Client::new()
            .post(&self.api_url)
            .bearer_auth(api_key)
            .multipart(form)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("Ошибка API: {}", error_text));
        }

        let result: serde_json::Value = response.json().await?;
        result["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Нет поля text в ответе: {:?}", result))
    }

    pub fn is_available(&self) -> bool {
        self.api_key.is_some()
    }
}
