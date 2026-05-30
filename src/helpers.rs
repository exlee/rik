use anyhow::{Context, Result};

use crate::config::{ModelConfig, Provider};

/// Build an OpenAI Completions client from config.
pub fn build_openai(cfg: &ModelConfig) -> Result<rig::providers::openai::CompletionsClient> {
    let api_key = resolve_api_key(&cfg.api_key, Provider::OpenAI, Some("OPENAI_API_KEY"))?;
    let mut builder = rig::providers::openai::CompletionsClient::builder().api_key(&api_key);
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build OpenAI client"))
}

/// Build a generic OpenAI-compatible Completions client from config.
/// Requires `url` to be set in config.
pub fn build_openai_compatible(
    cfg: &ModelConfig,
) -> Result<rig::providers::openai::CompletionsClient> {
    let url = cfg
        .url
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Provider 'openaicompatible' requires 'url' in config"))?;
    let api_key = resolve_api_key(
        &cfg.api_key,
        Provider::OpenAiCompatible,
        Some("OPENAI_API_KEY"),
    )?;
    let builder = rig::providers::openai::CompletionsClient::builder()
        .base_url(url)
        .api_key(&api_key);
    Ok(builder
        .build()
        .expect("Failed to build OpenAI-compatible client"))
}

/// Build an Anthropic client from config.
pub fn build_anthropic(cfg: &ModelConfig) -> Result<rig::providers::anthropic::Client> {
    let api_key = resolve_api_key(&cfg.api_key, Provider::Anthropic, Some("ANTHROPIC_API_KEY"))?;
    let mut builder = rig::providers::anthropic::Client::builder().api_key(api_key);
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build Anthropic client"))
}

/// Build a Gemini client from config.
pub fn build_gemini(cfg: &ModelConfig) -> Result<rig::providers::gemini::Client> {
    let api_key = resolve_api_key(&cfg.api_key, Provider::Gemini, Some("GEMINI_API_KEY"))?;
    let mut builder = rig::providers::gemini::Client::builder().api_key(api_key);
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build Gemini client"))
}

/// Build an Ollama client from config.
/// Ollama does not require authentication by default.
pub fn build_ollama(cfg: &ModelConfig) -> Result<rig::providers::ollama::Client> {
    use rig::client::Nothing;
    let mut builder = rig::providers::ollama::Client::builder().api_key(Nothing);
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build Ollama client"))
}

/// Build an OpenRouter client from config.
pub fn build_openrouter(cfg: &ModelConfig) -> Result<rig::providers::openrouter::Client> {
    let api_key = resolve_api_key(
        &cfg.api_key,
        Provider::OpenRouter,
        Some("OPENROUTER_API_KEY"),
    )?;
    let mut builder = rig::providers::openrouter::Client::builder().api_key(api_key);
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build OpenRouter client"))
}

/// Build an xAI client from config.
pub fn build_xai(cfg: &ModelConfig) -> Result<rig::providers::xai::Client> {
    let api_key = resolve_api_key(&cfg.api_key, Provider::Xai, Some("XAI_API_KEY"))?;
    let mut builder = rig::providers::xai::Client::builder().api_key(api_key);
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build xAI client"))
}

/// Build a DeepSeek client from config.
pub fn build_deepseek(cfg: &ModelConfig) -> Result<rig::providers::deepseek::Client> {
    let api_key = resolve_api_key(&cfg.api_key, Provider::DeepSeek, Some("DEEPSEEK_API_KEY"))?;
    let mut builder = rig::providers::deepseek::Client::builder().api_key(api_key);
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build DeepSeek client"))
}

/// Build a Groq client from config.
pub fn build_groq(cfg: &ModelConfig) -> Result<rig::providers::groq::Client> {
    let api_key = resolve_api_key(&cfg.api_key, Provider::Groq, Some("GROQ_API_KEY"))?;
    let mut builder = rig::providers::groq::Client::builder().api_key(api_key);
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build Groq client"))
}

/// Build a Together client from config.
pub fn build_together(cfg: &ModelConfig) -> Result<rig::providers::together::Client> {
    let api_key = resolve_api_key(&cfg.api_key, Provider::Together, Some("TOGETHER_API_KEY"))?;
    let mut builder = rig::providers::together::Client::builder().api_key(api_key);
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build Together client"))
}

/// Build a Perplexity client from config.
pub fn build_perplexity(cfg: &ModelConfig) -> Result<rig::providers::perplexity::Client> {
    let api_key = resolve_api_key(
        &cfg.api_key,
        Provider::Perplexity,
        Some("PERPLEXITY_API_KEY"),
    )?;
    let mut builder = rig::providers::perplexity::Client::builder().api_key(api_key);
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build Perplexity client"))
}

/// Build a Mistral client from config.
pub fn build_mistral(cfg: &ModelConfig) -> Result<rig::providers::mistral::Client> {
    let api_key = resolve_api_key(&cfg.api_key, Provider::Mistral, Some("MISTRAL_API_KEY"))?;
    let mut builder = rig::providers::mistral::Client::builder().api_key(api_key);
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build Mistral client"))
}

/// Build a Cohere client from config.
pub fn build_cohere(cfg: &ModelConfig) -> Result<rig::providers::cohere::Client> {
    let api_key = resolve_api_key(&cfg.api_key, Provider::Cohere, Some("COHERE_API_KEY"))?;
    let mut builder = rig::providers::cohere::Client::builder().api_key(api_key);
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build Cohere client"))
}

/// Resolve the API key: explicit value > env var > error.
fn resolve_api_key(
    explicit: &Option<String>,
    provider: Provider,
    env_var: Option<&'static str>,
) -> Result<String> {
    if let Some(key) = explicit {
        return Ok(key.clone());
    }
    if let Some(var) = env_var
        && let Ok(key) = std::env::var(var)
    {
        return Ok(key);
    }
    let provider_name = format_provider_name(provider);
    anyhow::bail!(
        "No API key for {provider_name}. \
         Set it in config or via {} environment variable.",
        env_var.unwrap_or("<none>")
    )
}

fn format_provider_name(p: Provider) -> &'static str {
    match p {
        Provider::OpenAI => "OpenAI",
        Provider::Anthropic => "Anthropic",
        Provider::Gemini => "Gemini",
        Provider::Ollama => "Ollama",
        Provider::OpenRouter => "OpenRouter",
        Provider::Xai => "xAI",
        Provider::DeepSeek => "DeepSeek",
        Provider::Groq => "Groq",
        Provider::Together => "Together",
        Provider::Perplexity => "Perplexity",
        Provider::Mistral => "Mistral",
        Provider::Cohere => "Cohere",
        Provider::OpenAiCompatible => "OpenAI-compatible",
    }
}

// ---------------------------------------------------------------------------
// Path safety helpers
// ---------------------------------------------------------------------------

/// Validate that `raw` resolves within the current working directory.
/// Returns the validated relative path string, or an error describing why
/// the path was rejected.
pub fn validate_relative_path(raw: &str) -> Result<String> {
    let path = std::path::Path::new(raw);

    // Reject absolute paths
    if path.is_absolute() {
        anyhow::bail!("Absolute paths are not allowed: {}", raw);
    }

    // Normalize: resolve "." and ".." components against cwd,
    // then check the result still starts with cwd.
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("Unable to determine current directory: {e}"))?;

    let resolved = cwd.join(path)
        .canonicalize()
        .unwrap_or_else(|_| cwd.join(path));

    let cwd_canonical = cwd.canonicalize().unwrap_or(cwd.clone());

    if !resolved.starts_with(&cwd_canonical) {
        anyhow::bail!(
            "Path escapes current directory: {}",
            raw
        );
    }

    // Return the original relative path (already safe after checks above).
    Ok(raw.trim_start_matches("./").to_string())
}

// ---------------------------------------------------------------------------
// Glob / diff helpers (unchanged)
// ---------------------------------------------------------------------------

/// Expand one or more comma-separated glob patterns into a list of file paths.
/// Each segment is trimmed before expansion.
pub fn expand_glob(pattern: &str) -> Result<Vec<std::path::PathBuf>> {
    let mut results: Vec<std::path::PathBuf> = Vec::new();

    for part in pattern.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // If the part is a literal existing file, return it directly.
        let path = std::path::Path::new(part);
        if path.is_file() {
            results.push(path.to_path_buf());
            continue;
        }

        // Otherwise treat as a glob pattern.
        use glob::glob;
        let matches = glob(part).with_context(|| format!("Invalid glob pattern: {part}"))?;
        results.extend(
            matches
                .filter_map(|entry| entry.ok())
                .filter(|p| p.is_file()),
        );
    }

    Ok(results)
}

/// Diff tools to try in order when none is configured.
const DIFF_TOOL_CANDIDATES: &[&str] = &["difft", "delta", "diff"];

/// Resolve the diff command to use.
///
/// If `configured` is `Some`, returns it as-is (user is responsible for including
/// `$pre`/`$post`). Otherwise auto-detects the first available tool and builds
/// a default args list: `["<tool>", "$pre", "$post"]`.
pub fn resolve_diff_tool(configured: Option<&Vec<String>>) -> Option<Vec<String>> {
    if let Some(args) = configured
        && !args.is_empty()
    {
        return Some(args.clone());
    }
    for candidate in DIFF_TOOL_CANDIDATES {
        if which_exists(candidate) {
            return Some(vec![
                candidate.to_string(),
                "$pre".to_string(),
                "$post".to_string(),
            ]);
        }
    }
    None
}

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run a diff command, replacing `$pre` and `$post` placeholders with temp file paths.
///
/// When stdout is a TTY, runs the command with inherited stdout/stderr so the
/// diff tool can detect the terminal and produce colored output. Returns an
/// empty string in that case since output goes directly to the terminal.
/// When not a TTY (piped), captures output and returns it as a string.
pub fn run_diff(args: &[String], label: &str, old_content: &str, new_content: &str) -> String {
    use std::io::IsTerminal;

    let dir = tempfile::tempdir().ok();
    let dir_path = dir
        .as_ref()
        .map(|d: &tempfile::TempDir| d.path())
        .unwrap_or_else(|| std::path::Path::new("/tmp"));
    let pre_path = dir_path.join(format!("{label}.old"));
    let post_path = dir_path.join(format!("{label}.new"));

    let _ = std::fs::write(&pre_path, old_content);
    let _ = std::fs::write(&post_path, new_content);

    let pre_str = pre_path.to_string_lossy();
    let post_str = post_path.to_string_lossy();

    let resolved: Vec<String> = args
        .iter()
        .map(|a| a.replace("$pre", &pre_str).replace("$post", &post_str))
        .collect();

    let is_tty = std::io::stdout().is_terminal();

    // Temp files cleaned up when `dir` drops.
    if is_tty {
        // Inherit stdout/stderr so the diff tool sees a real TTY and uses colors.
        std::process::Command::new(&resolved[0])
            .args(&resolved[1..])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .map(|_| String::new())
            .unwrap_or_else(|e| format!("Failed to run diff tool '{}': {e}", resolved[0]))
    } else {
        let output = std::process::Command::new(&resolved[0])
            .args(&resolved[1..])
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let mut result = stdout.to_string();
                if !stderr.is_empty() {
                    if !result.is_empty() {
                        result.push('\n');
                    }
                    result.push_str(&stderr);
                }
                result
            }
            Err(e) => format!("Failed to run diff tool '{}': {e}", resolved[0]),
        }
    }
}
