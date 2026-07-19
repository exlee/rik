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

/// Build a ChatGPT subscription client from config.
///
/// Authenticates against the ChatGPT backend via OAuth device flow by default;
/// no API key is required. Tokens are cached at `~/.config/rik/chatgpt-auth.json`
/// and refreshed automatically. When `api_key` (or `CHATGPT_ACCESS_TOKEN`) is
/// set, it is used as a raw access token instead of triggering OAuth.
pub fn build_chatgpt(cfg: &ModelConfig) -> Result<rig::providers::chatgpt::Client> {
    let auth_file = dirs::home_dir()
        .map(|home| home.join(".config").join("rik").join("chatgpt-auth.json"))
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory for ChatGPT auth cache"))?;

    let access_token = cfg
        .api_key
        .clone()
        .or_else(|| std::env::var("CHATGPT_ACCESS_TOKEN").ok().filter(|s| !s.is_empty()));

    let builder = rig::providers::chatgpt::Client::builder();
    let builder = if let Some(token) = access_token {
        builder.api_key(token)
    } else {
        builder.oauth()
    };
    let mut builder = builder
        .auth_file(&auth_file)
        .on_device_code(|prompt| {
            println!(
                "\nChatGPT sign-in required:\n  1) Visit: {}\n  2) Enter code: {}\n\
                 Waiting for authorization (do not share this code)...\n",
                prompt.verification_uri, prompt.user_code,
            );
        });
    if let Some(url) = &cfg.url {
        builder = builder.base_url(url.as_str());
    }
    Ok(builder.build().expect("Failed to build ChatGPT client"))
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
        Provider::ChatGPT => "ChatGPT",
        Provider::OpenAiCompatible => "OpenAI-compatible",
    }
}

// ---------------------------------------------------------------------------
// Glob / diff helpers (unchanged)
// ---------------------------------------------------------------------------

/// Derive the directory watched by a pattern.
///
/// Relative patterns watch the current working directory. Absolute patterns
/// watch their absolute directory scope. Multiple comma-separated patterns use
/// their common ancestor.
pub fn watched_directory(pattern: &str) -> Result<std::path::PathBuf> {
    let mut watched: Option<std::path::PathBuf> = None;

    for part in pattern.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let path = std::path::Path::new(part);
        let root = if path.is_absolute() {
            absolute_pattern_directory(path)?
        } else {
            std::env::current_dir()
                .context("Failed to determine current working directory")?
                .canonicalize()
                .context("Failed to resolve current working directory")?
        };

        watched = Some(match watched {
            Some(current) => common_ancestor(&current, &root),
            None => root,
        });
    }

    watched.ok_or_else(|| anyhow::anyhow!("Pattern must not be empty"))
}

fn absolute_pattern_directory(path: &std::path::Path) -> Result<std::path::PathBuf> {
    let part = path.display();
    let mut root = std::path::PathBuf::new();
    let mut has_glob = false;
    for component in path.components() {
        if component
            .as_os_str()
            .to_string_lossy()
            .chars()
            .any(|c| matches!(c, '*' | '?' | '['))
        {
            has_glob = true;
            break;
        }
        root.push(component.as_os_str());
    }
    if !has_glob && !root.is_dir() {
        root.pop();
    }
    while !root.is_dir() {
        if !root.pop() {
            anyhow::bail!("Could not determine watched directory for pattern: {part}");
        }
    }
    root.canonicalize()
        .with_context(|| format!("Failed to resolve watched directory: {}", root.display()))
}

fn common_ancestor(a: &std::path::Path, b: &std::path::Path) -> std::path::PathBuf {
    let mut common = std::path::PathBuf::new();
    for (a, b) in a.components().zip(b.components()) {
        if a != b {
            break;
        }
        common.push(a.as_os_str());
    }
    common
}

/// Expand one or more comma-separated glob patterns into a list of file paths.
/// Each segment is trimmed before expansion.
pub fn expand_glob(
    app_state: &crate::state::AppState,
    pattern: &str,
    no_ignore: bool,
) -> Result<Vec<std::path::PathBuf>> {
    enum Matcher {
        Literal(std::path::PathBuf),
        Glob(glob::Pattern),
    }

    let mut matchers = Vec::new();

    for part in pattern.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let path = app_state.resolve_path(part)?;
        if path.is_file() {
            matchers.push(Matcher::Literal(path));
        } else {
            let glob = glob::Pattern::new(path.to_string_lossy().as_ref())
                .with_context(|| format!("Invalid glob pattern: {part}"))?;
            matchers.push(Matcher::Glob(glob));
        }
    }

    let mut builder = ignore::WalkBuilder::new(&app_state.path);
    builder
        .hidden(false)
        .git_ignore(!no_ignore)
        .git_global(!no_ignore)
        .git_exclude(!no_ignore)
        .ignore(!no_ignore)
        .parents(!no_ignore);

    let mut results = Vec::new();
    for entry in builder.build().filter_map(|entry| entry.ok()) {
        if !entry
            .file_type()
            .is_some_and(|file_type| file_type.is_file())
        {
            continue;
        }

        let path = entry.path();
        if is_binary_file(path) {
            continue;
        }

        let matched = matchers.iter().any(|matcher| match matcher {
            Matcher::Literal(literal) => path == literal,
            Matcher::Glob(glob) => glob.matches_path(path),
        });
        if matched {
            results.push(path.to_path_buf());
        }
    }

    Ok(results)
}

pub fn is_binary_file(path: &std::path::Path) -> bool {
    const SAMPLE_SIZE: usize = 8192;

    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return true,
    };
    let mut buffer = [0u8; SAMPLE_SIZE];
    let read = match std::io::Read::read(&mut file, &mut buffer) {
        Ok(read) => read,
        Err(_) => return true,
    };
    buffer[..read].contains(&0)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watched_directory_uses_absolute_glob_prefix_even_without_matches() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let src = dir.path().join("project").join("src");
        std::fs::create_dir_all(&src)?;
        let pattern = src.join("**").join("*.rs");

        assert_eq!(
            watched_directory(pattern.to_string_lossy().as_ref())?,
            src.canonicalize()?
        );
        Ok(())
    }

    #[test]
    fn watched_directory_uses_cwd_for_relative_pattern() -> anyhow::Result<()> {
        assert_eq!(
            watched_directory("src/**/*.rs")?,
            std::env::current_dir()?.canonicalize()?
        );
        Ok(())
    }

    #[test]
    fn watched_directory_uses_cwd_for_dot_relative_pattern() -> anyhow::Result<()> {
        assert_eq!(
            watched_directory("./src/**/*.rs")?,
            std::env::current_dir()?.canonicalize()?
        );
        Ok(())
    }

    #[test]
    fn watched_directory_uses_common_ancestor_for_multiple_patterns() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join("src"))?;
        std::fs::create_dir_all(project.join("tests"))?;
        let pattern = format!(
            "{}/**/*.rs,{}/**/*.rs",
            project.join("src").display(),
            project.join("tests").display()
        );

        assert_eq!(watched_directory(&pattern)?, project.canonicalize()?);
        Ok(())
    }

    #[test]
    fn expand_glob_honors_ignore_files_by_default() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project)?;
        std::fs::write(project.join(".ignore"), "ignored.txt\n")?;
        std::fs::write(project.join("kept.txt"), "rik: keep\n")?;
        std::fs::write(project.join("ignored.txt"), "rik: ignore\n")?;
        let state = crate::state::AppState::new(project.clone(), crate::config::Config::default())?;
        let pattern = project.join("*.txt");

        let files = expand_glob(&state, pattern.to_string_lossy().as_ref(), false)?;

        assert_eq!(files, vec![project.join("kept.txt").canonicalize()?]);
        Ok(())
    }

    #[test]
    fn expand_glob_can_disable_ignore_files() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project)?;
        std::fs::write(project.join(".ignore"), "ignored.txt\n")?;
        std::fs::write(project.join("kept.txt"), "rik: keep\n")?;
        std::fs::write(project.join("ignored.txt"), "rik: include\n")?;
        let state = crate::state::AppState::new(project.clone(), crate::config::Config::default())?;
        let pattern = project.join("*.txt");

        let mut files = expand_glob(&state, pattern.to_string_lossy().as_ref(), true)?;
        files.sort();

        assert_eq!(
            files,
            vec![
                project.join("ignored.txt").canonicalize()?,
                project.join("kept.txt").canonicalize()?
            ]
        );
        Ok(())
    }

    #[test]
    fn expand_glob_skips_binary_files() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project)?;
        std::fs::write(project.join("text.txt"), "rik: keep\n")?;
        std::fs::write(project.join("binary.txt"), b"rik: nope\0still nope")?;
        let state = crate::state::AppState::new(project.clone(), crate::config::Config::default())?;
        let pattern = project.join("*.txt");

        let files = expand_glob(&state, pattern.to_string_lossy().as_ref(), true)?;

        assert_eq!(files, vec![project.join("text.txt").canonicalize()?]);
        Ok(())
    }
}
