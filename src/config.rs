use anyhow::Context;
use serde::Deserialize;

/// Supported LLM providers.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    OpenAI,
    Anthropic,
    Gemini,
    Ollama,
    OpenRouter,
    Xai,
    DeepSeek,
    Groq,
    Together,
    Perplexity,
    Mistral,
    Cohere,
    /// Generic OpenAI-compatible endpoint (custom URL required).
    #[serde(alias = "open_ai_compatible", alias = "openaicompatible")]
    OpenAiCompatible,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub model: ModelConfig,
    /// Diff command as a list of arguments. Use "$pre" and "$post" as
    /// placeholders for the pre/post change temp file paths.
    /// Example: ["difft", "--color", "always", "$pre", "$post"]
    /// When unset, auto-detects difft, delta, or diff.
    #[serde(default)]
    pub diff_tool: Option<Vec<String>>,
    /// Enable personality mode. When enabled, Rik will be more chatty about his work.
    #[serde(default)]
    pub personality: bool,
}

#[derive(Deserialize, Debug)]
pub struct ModelConfig {
    /// Which provider to use. Defaults to "openai".
    ///
    /// Supported values: openai, anthropic, gemini, ollama, openrouter, xai,
    /// deepseek, groq, together, perplexity, mistral, cohere, openaicompatible.
    #[serde(default)]
    pub provider: Provider,

    /// API base URL. Optional — when omitted the provider default is used.
    /// Required for `openaicompatible`.
    ///
    /// Examples:
    ///   openai:          "https://api.openai.com/v1"       (default)
    ///   anthropic:       "https://api.anthropic.com"        (default)
    ///   gemini:          "https://generativelanguage.googleapis.com" (default)
    ///   ollama:          "http://localhost:11434"           (default)
    ///   openaicompatible: "<your-endpoint>"                 (required)
    pub url: Option<String>,

    /// API key. Read from environment variable when omitted.
    ///
    /// Environment variables checked per provider:
    ///   openai:          OPENAI_API_KEY
    ///   anthropic:       ANTHROPIC_API_KEY
    ///   gemini:          GEMINI_API_KEY
    ///   ollama:          (no key needed, ignored)
    ///   openrouter:      OPENROUTER_API_KEY
    ///   xai:             XAI_API_KEY
    ///   deepseek:        DEEPSEEK_API_KEY
    ///   groq:            GROQ_API_KEY
    ///   together:        TOGETHER_API_KEY
    ///   perplexity:      PERPLEXITY_API_KEY
    ///   mistral:         MISTRAL_API_KEY
    ///   cohere:          COHERE_API_KEY
    ///   openaicompatible: OPENAI_API_KEY
    pub api_key: Option<String>,

    /// Model name (e.g. "gpt-4o", "claude-sonnet-4-20250514", "gemini-2.5-pro").
    pub model: String,
}

fn find_config_path() -> Option<std::path::PathBuf> {
    // Try ~/.config/rik/rik.toml first (Linux-style, works everywhere)
    if let Some(home) = dirs::home_dir() {
        let linux_style = home.join(".config").join("rik").join("rik.toml");
        if linux_style.exists() {
            return Some(linux_style);
        }
    }

    // Fall back to platform-specific config dir
    dirs::config_dir().map(|d| d.join("rik").join("rik.toml"))
}

pub fn load() -> anyhow::Result<Config> {
    let config_path = find_config_path()
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
    let contents = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config at {}", config_path.display()))?;
    toml::from_str(&contents).context("Failed to parse config.toml")
}
