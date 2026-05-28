use anyhow::Context;
use rig::providers::openai;

use crate::config::ModelConfig;

pub fn make_completion_client(cfg: &ModelConfig) -> openai::CompletionsClient {
    openai::Client::builder()
        .base_url(&cfg.completion_url)
        .api_key(&cfg.completion_api_key)
        .build()
        .expect("Failed to build completion client")
        .completions_api()
}

pub fn expand_glob(pattern: &str) -> anyhow::Result<Vec<std::path::PathBuf>> {
    // If the pattern is a literal existing file, return it directly.
    let path = std::path::Path::new(pattern);
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }

    // Otherwise treat it as a glob pattern.
    use glob::glob;
    Ok(glob(pattern)
        .context("Invalid glob pattern")?
        .filter_map(|entry| entry.ok())
        .filter(|p| p.is_file())
        .collect())
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
