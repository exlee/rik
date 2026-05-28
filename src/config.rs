use anyhow::Context;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub model: ModelConfig,
    /// Diff command as a list of arguments. Use "$pre" and "$post" as
    /// placeholders for the pre/post change temp file paths.
    /// Example: ["difft", "--color", "always", "$pre", "$post"]
    /// When unset, auto-detects difft, delta, or diff.
    #[serde(default)]
    pub diff_tool: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
pub struct ModelConfig {
    pub completion_url: String,
    pub completion_api_key: String,
    pub completion_model: String,
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
