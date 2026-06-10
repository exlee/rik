use anyhow::Context;
use serde::Deserialize;
use std::fmt;

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

impl fmt::Display for Provider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::OpenAI => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Gemini",
            Self::Ollama => "Ollama",
            Self::OpenRouter => "OpenRouter",
            Self::Xai => "xAI",
            Self::DeepSeek => "DeepSeek",
            Self::Groq => "Groq",
            Self::Together => "Together",
            Self::Perplexity => "Perplexity",
            Self::Mistral => "Mistral",
            Self::Cohere => "Cohere",
            Self::OpenAiCompatible => "OpenAI-compatible",
        })
    }
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
    #[serde(default = "bool_true")]
    pub marker_limits_edition_range: bool,
}

pub fn bool_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: ModelConfig::default(),
            diff_tool: None,
            personality: false,
            marker_limits_edition_range: true,
        }
    }
}

#[derive(Deserialize, Debug, Default)]
pub struct ModelConfig {
    /// Which provider to use.
    ///
    /// Supported values: openai, anthropic, gemini, ollama, openrouter, xai,
    /// deepseek, groq, together, perplexity, mistral, cohere, openaicompatible.
    pub provider: Provider,

    /// API base URL. Optional — when omitted the provider default is used.
    /// Required for `openaicompatible`.
    ///
    /// Examples:
    /// ```text
    /// openai:           "https://api.openai.com/v1"                 (default)
    /// anthropic:        "https://api.anthropic.com"                  (default)
    /// gemini:           "https://generativelanguage.googleapis.com" (default)
    /// ollama:           "http://localhost:11434"                     (default)
    /// openaicompatible: "<your-endpoint>"                            (required)
    /// ```
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

pub fn load(model: Option<&str>) -> anyhow::Result<Config> {
    let config_path = find_config_path()
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
    let contents = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config at {}", config_path.display()))?;
    parse(&contents, model).context("Failed to parse config.toml")
}

fn parse(contents: &str, model: Option<&str>) -> anyhow::Result<Config> {
    let mut config: toml::Value = toml::from_str(contents)?;

    if let Some(model) = model {
        let selected = select_model(&config, model)?;
        config
            .as_table_mut()
            .context("Config root must be a TOML table")?
            .insert("model".to_owned(), selected);
    }

    config.try_into().map_err(Into::into)
}

fn select_model(config: &toml::Value, name: &str) -> anyhow::Result<toml::Value> {
    let path: Vec<_> = name.split('.').collect();
    if path.is_empty() || path.iter().any(|part| part.is_empty()) {
        anyhow::bail!("Invalid model profile '{name}'");
    }

    for root in ["models", "model"] {
        if let Some(selected) = select_model_from_root(config, root, &path) {
            return Ok(selected);
        }
    }

    anyhow::bail!("Model profile '{name}' not found under [models] or [model]");
}

fn select_model_from_root(config: &toml::Value, root: &str, path: &[&str]) -> Option<toml::Value> {
    let mut current = config.get(root)?.as_table()?;
    let mut selected = toml::map::Map::new();

    inherit_model_fields(&mut selected, current);
    for part in path {
        current = current.get(*part)?.as_table()?;
        inherit_model_fields(&mut selected, current);
    }

    Some(toml::Value::Table(selected))
}

fn inherit_model_fields(
    selected: &mut toml::map::Map<String, toml::Value>,
    current: &toml::map::Map<String, toml::Value>,
) {
    for (key, value) in current {
        if !value.is_table() {
            selected.insert(key.clone(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_existing_single_model_config() {
        let config = parse(
            r#"
                personality = true
                [model]
                provider = "anthropic"
                model = "claude"
            "#,
            None,
        )
        .unwrap();

        assert_eq!(config.model.provider, Provider::Anthropic);
        assert_eq!(config.model.model, "claude");
        assert!(config.personality);
    }

    #[test]
    fn selects_nested_model_and_inherits_parent_settings() {
        let config = parse(
            r#"
                [models.openrouter]
                provider = "openrouter"
                api_key = "aaaa"

                [models.openrouter.gpt120]
                model = "gpt-120:turbo"
            "#,
            Some("openrouter.gpt120"),
        )
        .unwrap();

        assert_eq!(config.model.provider, Provider::OpenRouter);
        assert_eq!(config.model.api_key.as_deref(), Some("aaaa"));
        assert_eq!(config.model.model, "gpt-120:turbo");
    }

    #[test]
    fn child_model_settings_override_parent_settings() {
        let config = parse(
            r#"
                [models.openrouter]
                provider = "openrouter"
                api_key = "parent"
                model = "default"

                [models.openrouter.fast]
                api_key = "child"
                model = "fast"
            "#,
            Some("openrouter.fast"),
        )
        .unwrap();

        assert_eq!(config.model.api_key.as_deref(), Some("child"));
        assert_eq!(config.model.model, "fast");
    }

    #[test]
    fn supports_named_profiles_under_model_table() {
        let config = parse(
            r#"
                [model.zai]
                provider = "openai"
                model = "zai-model"
            "#,
            Some("zai"),
        )
        .unwrap();

        assert_eq!(config.model.provider, Provider::OpenAI);
        assert_eq!(config.model.model, "zai-model");
    }

    #[test]
    fn reports_missing_model_profile() {
        let error = parse(
            "[models.openrouter]\napi_key = \"aaaa\"",
            Some("openrouter.missing"),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Model profile 'openrouter.missing' not found")
        );
    }

    #[test]
    fn requires_provider_in_selected_model_profile() {
        let error = parse(
            r#"
                [models.zai]
                model = "zai-model"
            "#,
            Some("zai"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("missing field `provider`"));
    }
}
