//! Provider settings (provider, base URL, model and request options),
//! persisted as JSON in the user's application-support directory. API keys
//! live in the Keychain instead (see [`super::keychain`]), one per provider.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// AI providers with an OpenAI-compatible chat-completions API.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    OpenAi,
    Anthropic,
    OpenRouter,
    Groq,
    Ollama,
    LmStudio,
    Custom,
}

impl Provider {
    pub const ALL: [Provider; 7] = [
        Provider::OpenAi,
        Provider::Anthropic,
        Provider::OpenRouter,
        Provider::Groq,
        Provider::Ollama,
        Provider::LmStudio,
        Provider::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Provider::OpenAi => "OpenAI",
            Provider::Anthropic => "Anthropic (Claude)",
            Provider::OpenRouter => "OpenRouter",
            Provider::Groq => "Groq",
            Provider::Ollama => "Ollama (local)",
            Provider::LmStudio => "LM Studio (local)",
            Provider::Custom => "Other (OpenAI-compatible)",
        }
    }

    pub fn base_url(self) -> &'static str {
        match self {
            Provider::OpenAi => "https://api.openai.com/v1",
            Provider::Anthropic => "https://api.anthropic.com/v1",
            Provider::OpenRouter => "https://openrouter.ai/api/v1",
            Provider::Groq => "https://api.groq.com/openai/v1",
            Provider::Ollama => "http://localhost:11434/v1",
            Provider::LmStudio => "http://localhost:1234/v1",
            Provider::Custom => "",
        }
    }

    /// A good current default: fast and inexpensive, smart enough for
    /// formulas and data questions. Local servers have no default, since
    /// it depends on what's installed ("Load models" lists them).
    pub fn default_model(self) -> &'static str {
        match self {
            Provider::OpenAi => "gpt-6-luna",
            Provider::Anthropic => "claude-haiku-5-5",
            Provider::OpenRouter => "openai/gpt-6-luna",
            Provider::Groq => "openai/gpt-oss-120b",
            Provider::Ollama | Provider::LmStudio | Provider::Custom => "",
        }
    }

    /// Local servers don't need a key.
    pub fn needs_key(self) -> bool {
        !matches!(self, Provider::Ollama | Provider::LmStudio)
    }

    /// Where to get a key, for the settings screen.
    pub fn key_url(self) -> Option<&'static str> {
        match self {
            Provider::OpenAi => Some("https://platform.openai.com/api-keys"),
            Provider::Anthropic => Some("https://platform.claude.com/settings/keys"),
            Provider::OpenRouter => Some("https://openrouter.ai/keys"),
            Provider::Groq => Some("https://console.groq.com/keys"),
            _ => None,
        }
    }

    /// The Keychain account this provider's key is stored under. OpenAI
    /// keeps the original name, so keys saved by older versions still work.
    pub fn key_account(self) -> &'static str {
        match self {
            Provider::OpenAi => "ai-api-key",
            Provider::Anthropic => "ai-api-key-anthropic",
            Provider::OpenRouter => "ai-api-key-openrouter",
            Provider::Groq => "ai-api-key-groq",
            Provider::Ollama => "ai-api-key-ollama",
            Provider::LmStudio => "ai-api-key-lmstudio",
            Provider::Custom => "ai-api-key-custom",
        }
    }

    /// OpenAI's newer models take `max_completion_tokens` (and reject
    /// `max_tokens`); everyone else still uses `max_tokens`.
    pub fn max_tokens_param(self) -> &'static str {
        match self {
            Provider::OpenAi => "max_completion_tokens",
            _ => "max_tokens",
        }
    }
}

/// How hard a reasoning model thinks before answering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub provider: Provider,
    pub base_url: String,
    pub model: String,
    /// `None` uses the model's default (some models accept nothing else).
    pub temperature: Option<f32>,
    /// Cap on the reply length, in tokens. `None` lets the provider decide.
    pub max_tokens: Option<u32>,
    /// For reasoning models; `None` leaves it to the model.
    pub reasoning_effort: Option<Effort>,
    /// How long to wait for a reply, in seconds.
    pub timeout_secs: u64,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self::for_provider(Provider::OpenAi)
    }
}

/// The model older versions defaulted to; settings still on it move to the
/// current default.
const OLD_DEFAULT_MODEL: &str = "gpt-4o-mini";

fn config_path() -> Option<PathBuf> {
    crate::config::file("settings.json")
}

impl AiSettings {
    pub fn for_provider(provider: Provider) -> Self {
        Self {
            provider,
            base_url: provider.base_url().into(),
            model: provider.default_model().into(),
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            timeout_secs: 180,
        }
    }

    /// Switch provider, keeping the request options.
    pub fn set_provider(&mut self, provider: Provider) {
        if provider == self.provider {
            return;
        }
        self.provider = provider;
        if provider != Provider::Custom {
            self.base_url = provider.base_url().into();
        }
        self.model = provider.default_model().into();
    }

    pub fn load() -> Self {
        // Tests start from the defaults, not the user's own settings.
        if cfg!(test) {
            return Self::default();
        }
        let mut s: Self = config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        s.migrate();
        s
    }

    /// Settings saved by older versions only had a base URL and a model.
    fn migrate(&mut self) {
        if self.provider == Provider::OpenAi
            && self.base_url.trim_end_matches('/') != Provider::OpenAi.base_url()
        {
            self.provider = Provider::ALL
                .into_iter()
                .find(|p| {
                    *p != Provider::Custom && self.base_url.trim_end_matches('/') == p.base_url()
                })
                .unwrap_or(Provider::Custom);
        }
        if self.model == OLD_DEFAULT_MODEL && self.provider == Provider::OpenAi {
            self.model = Provider::OpenAi.default_model().into();
        }
        if self.timeout_secs == 0 {
            self.timeout_secs = 180;
        }
    }

    pub fn save(&self) -> Result<(), String> {
        if cfg!(test) {
            return Ok(());
        }
        let path = config_path().ok_or("No home directory")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_are_migrated() {
        let mut s: AiSettings = serde_json::from_str(
            r#"{"base_url":"https://api.openai.com/v1","model":"gpt-4o-mini"}"#,
        )
        .unwrap();
        s.migrate();
        assert_eq!(s.provider, Provider::OpenAi);
        assert_eq!(s.model, "gpt-6-luna");
        assert_eq!(s.temperature, None);
        assert_eq!(s.timeout_secs, 180);

        let mut s: AiSettings =
            serde_json::from_str(r#"{"base_url":"http://localhost:11434/v1/","model":"llama3.2"}"#)
                .unwrap();
        s.migrate();
        assert_eq!(
            (s.provider, s.model.as_str()),
            (Provider::Ollama, "llama3.2")
        );

        let mut s: AiSettings =
            serde_json::from_str(r#"{"base_url":"https://example.com/v1","model":"m"}"#).unwrap();
        s.migrate();
        assert_eq!(s.provider, Provider::Custom);
    }
}
