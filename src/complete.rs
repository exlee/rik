use anyhow::Context;
use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::streaming::{StreamedAssistantContent, StreamingPrompt};
use std::io::Write;

use crate::config::{Config, ModelConfig, Provider};
use crate::helpers::{expand_glob, resolve_diff_tool, run_diff};
use crate::markers::MarkerKind;
use crate::{cleanup, personality, raii, tools};

// ---------------------------------------------------------------------------
// Shared processing logic parameterized over provider client types via a macro.
// Each provider has its own concrete Client + CompletionModel types so we can't
// trait-object over them.  The macro generates typed wrappers per provider.
// ---------------------------------------------------------------------------

macro_rules! define_provider_dispatch {
    (
        $(
            $variant:ident($fn_name:ident) => $client_type:ty
        ),* $(,)?
    ) => {
        /// Dispatch to the correct handler based on the configured provider.
        async fn scan_and_complete_dispatch(
            cfg: &ModelConfig,
            alias: &str,
            diff_tool: Option<&Vec<String>>,
            pattern: &str,
            verbose: bool,
            personality: bool,
        ) -> anyhow::Result<usize> {
            match cfg.provider {
                $(
                    Provider::$variant => {
                        let client = crate::helpers::$fn_name(cfg)
                            .with_context(|| format!("Failed to build {:?} client", Provider::$variant))?;
                        process_scan_and_complete::<$client_type>(
                            &client,
                            &cfg.model,
                            alias,
                            diff_tool,
                            pattern,
                            verbose,
                            personality,
                        ).await
                    }
                )*
            }
        }
    };
}

define_provider_dispatch!(
    OpenAI(build_openai)              => rig::providers::openai::CompletionsClient,
    Anthropic(build_anthropic)        => rig::providers::anthropic::Client,
    Gemini(build_gemini)              => rig::providers::gemini::Client,
    Ollama(build_ollama)              => rig::providers::ollama::Client,
    OpenRouter(build_openrouter)      => rig::providers::openrouter::Client,
    Xai(build_xai)                    => rig::providers::xai::Client,
    DeepSeek(build_deepseek)          => rig::providers::deepseek::Client,
    Groq(build_groq)                  => rig::providers::groq::Client,
    Together(build_together)          => rig::providers::together::Client,
    Perplexity(build_perplexity)      => rig::providers::perplexity::Client,
    Mistral(build_mistral)            => rig::providers::mistral::Client,
    Cohere(build_cohere)              => rig::providers::cohere::Client,
    OpenAiCompatible(build_openai_compatible) => rig::providers::openai::CompletionsClient,
);

/// Return the file extension or "unknown".
fn file_extension(path: &std::path::Path) -> &str {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown")
}

/// Extract a window of lines around `center_line` (1-based).
/// Returns the lines with line numbers prefixed.
fn surrounding_lines(content: &str, center_line: usize, radius: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let start = center_line.saturating_sub(radius + 1);
    let end = (center_line + radius).min(lines.len());
    lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:>4} | {}", start + i + 1, line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Preamble injected into the agent for file-completion mode.
fn make_preamble(alias: &str) -> String {
    format!("\
You are an in-place editor. A file contains '{alias}: <instruction>' markers that \
must be replaced with real content. The file is a working document (code, prose, \
config, etc.) and your edits must keep it coherent and correct.

There may be MULTIPLE markers in the file. Process ALL of them in a single pass. \
Do NOT stop after the first one.

Tools:
- read_file: read other files for context (types, imports, conventions).
- edit_file: replace exact text in the target file. old_text must be unique.
- list_files: discover files in the project.

Rules:
- Study the surrounding lines BEFORE editing. Your replacement must fit the \
  existing style, indentation, language, and intent of the file.
- If the file is code, respect existing imports, types, and variable names. \
  Add needed imports only if you can verify they are missing.
- If you are unsure about conventions, read nearby files for reference.
- You may make MULTIPLE edit_file calls if the change requires touching more \
  than one spot (e.g. adding an import AND inserting code).
- Each edit_file call must have a unique old_text match.
- Do NOT add comments explaining what you did. Just make the edit.
- Do NOT echo back the file contents. The edit_file call IS your output.
- After editing, provide a SHORT summary of what you changed (under 250 characters). \
  A diff of your changes will be shown to the user separately, so focus on intent, not line-by-line description.")
}

/// Remove context-only marker lines from a file.
///
/// Re-scans the current file content so line positions are accurate even after
/// earlier edits shifted things. Only removes single-line context markers
/// (`rik: /.../`). Multi-line blocks are never context markers.
fn remove_context_markers(
    file_path: &std::path::Path,
    alias: &str,
    _initial_context: &[&crate::markers::FoundMarker],
) -> anyhow::Result<()> {
    let mut content = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read for cleanup: {}", file_path.display()))?;

    // Re-scan so positions reflect any shifts from prior edits.
    let markers = crate::markers::find_markers(&content, alias);
    let context_positions: Vec<usize> = markers.iter()
        .filter(|m| m.kind == MarkerKind::Context)
        .map(|m| m.start_line - 1) // Convert 1-based to 0-based index.
        .collect();

    if context_positions.is_empty() {
        return Ok(());
    }

    println!(
        "Removing {} context marker line(s) from {}",
        context_positions.len(),
        file_path.display()
    );

    // Build the new content with context lines removed.
    let lines: Vec<&str> = content.lines().collect();
    let had_trailing_newline = content.ends_with('\n');
    let kept: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(idx, _)| !context_positions.contains(idx))
        .map(|(_, line)| *line)
        .collect();

    let mut new_content = kept.join("\n");
    // Preserve trailing newline if original had one.
    if had_trailing_newline && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    content = new_content;

    std::fs::write(file_path, &content)
        .with_context(|| format!("Failed to write cleaned file: {}", file_path.display()))?;

    Ok(())
}

async fn process_file_markers<C>(
    comp_client: &C,
    model_name: &str,
    alias: &str,
    diff_tool: Option<&Vec<String>>,
    file_path: &std::path::Path,
    verbose: bool,
    personality: bool,
) -> anyhow::Result<usize>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    let content_before = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read: {}", file_path.display()))?;

    let all_markers = crate::markers::find_markers(&content_before, alias);
    if all_markers.is_empty() {
        return Ok(0);
    }

    let halt_tags = [format!("!{alias}"), format!("{alias}!")];
    for halt_tag in halt_tags {
        if content_before.lines().any(|line| line.contains(&halt_tag)) {
            println!(
                "Found {} marker(s) in {} — skipped ({} guard present)",
                all_markers.len(),
                file_path.display(),
                halt_tag
            );
            return Ok(0);
        }
    }

    // Separate task markers (agent must act) from context markers (supplementary info).
    let task_markers: Vec<_> = all_markers.iter()
        .filter(|m| m.kind == MarkerKind::Task)
        .collect();
    let context_markers: Vec<_> = all_markers.iter()
        .filter(|m| m.kind == MarkerKind::Context)
        .collect();

    // If there are no task markers (only context), nothing to do but clean up.
    if task_markers.is_empty() {
        remove_context_markers(file_path, alias, &context_markers)?;
        return Ok(context_markers.len());
    }

    println!(
        "Found {} marker(s) in {} ({} task{}, {} context)",
        all_markers.len(),
        file_path.display(),
        task_markers.len(),
        if task_markers.len() != 1 { "s" } else { "" },
        context_markers.len(),
    );

    for m in &task_markers {
        println!("[{alias}] Task: {} (L{})", m.query, m.start_line);
    }
    for m in &context_markers {
        println!("[{alias}] Context: {} (L{})", m.query, m.start_line);
    }

    // Build a single prompt with all markers.
    let file_display = file_path.display().to_string();
    // Use a relative path for the target so tool output is clean.
    let target_rel: String = std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            file_path
                .strip_prefix(&cwd)
                .ok()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_else(|| file_display.clone());

    // Build blocks for task markers (things the agent must do).
    let tasks_block = task_markers
        .iter()
        .map(|m| {
            format!(
                "TASK at line {}: {alias}: {}\n\
                 Surrounding context:\n{}",
                m.start_line,
                m.query,
                surrounding_lines(&content_before, m.start_line, 5),
            )
        })
        .collect::<Vec<_>>()
        .join("\n---\n");

    // Build blocks for context markers (supplementary background info).
    let context_section = if context_markers.is_empty() {
        String::new()
    } else {
        let ctx_items = context_markers
            .iter()
            .map(|m| format!("- Line {}: {}", m.start_line, m.query))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "\nCONTEXT NOTES (background info, not tasks — these lines will be auto-removed after work):\n{ctx_items}"
        )
    };

    let prompt = format!(
        "Target file: {file_display}\n\
         File type: {}\n\
         Number of tasks to complete: {}\n\
         {}\n\
         {}\n\n\
         Read the file and any other context you need, then replace ALL task markers \
         with content that is coherent with the rest of the file according to each instruction.\n\
         Do NOT edit or remove the context note lines yourself; they will be cleaned up automatically.",
        file_extension(file_path),
        task_markers.len(),
        tasks_block,
        context_section,
    );

    let preamble = make_preamble(alias);
    let agent_builder = comp_client
        .agent(model_name)
        .preamble(&preamble)
        .tool(tools::ReadFileTool)
        .tool(tools::EditFileTool {
            target_path: target_rel,
            alias: alias.to_string(),
        })
        .tool(tools::ListFilesTool)
        .default_max_turns(20);

    let agent = agent_builder.build();

    if personality {
        personality::pre_work_personality(alias);
    }

    let _reverter = raii::FileReverter::new(file_path, alias)
        .with_context(|| format!("Failed to read {} for backup", file_path.display()))?;
    let mut stream = agent.stream_prompt(&prompt).await;
    let mut is_reasoning = false;
    let mut last_text = false;

    while let Some(item) = stream.next().await {
        if cleanup::is_shutting_down() || crate::keyboard::should_stop() {
            crate::keyboard::clear_stop();
            return Ok(0);
        }
        if !matches!(
            &item,
            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::Text(_text)
            ))
        ) {
            last_text = false;
        }
        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Reasoning(
                reasoning,
            ))) if verbose => {
                if !is_reasoning {
                    is_reasoning = true;
                    print!("\n    \x1b[90m// thinking...\x1b[0m\n");
                }
                print!("\x1b[90m{}\x1b[0m", reasoning.display_text());
                std::io::stdout().flush().ok();
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::ReasoningDelta { reasoning, .. },
            )) if verbose => {
                if !is_reasoning {
                    is_reasoning = true;
                    print!("\n    \x1b[90m// thinking...\x1b[0m\n");
                }
                print!("\x1b[90m{}\x1b[0m", reasoning);
                std::io::stdout().flush().ok();
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(text)))
                if verbose =>
            {
                if is_reasoning && verbose {
                    is_reasoning = false;
                    print!("\n    \x1b[0m");
                }
                if !last_text {
                    print!("");
                    last_text = true;
                }
                print!("{}", text.text);
                std::io::stdout().flush().ok();
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCall {
                tool_call,
                ..
            })) => {
                if is_reasoning && verbose {
                    is_reasoning = false;
                    print!("\n    \x1b[0m");
                }
                let msg = match tool_call.function.name.as_str() {
                    "list_files" => {
                        if let Some(obj) = tool_call.function.arguments.as_object() {
                            let mut parts = Vec::new();
                            if let Some(path) = obj.get("path").and_then(|v| v.as_str()) {
                                parts.push(format!("path={}", path));
                            }
                            if let Some(glob) = obj.get("glob").and_then(|v| v.as_str()) {
                                parts.push(format!("glob={}", glob));
                            }
                            parts.join(" ")
                        } else {
                            tool_call.function.arguments.to_string()
                        }
                    }
                    "read_file" => {
                        if let Some(obj) = tool_call.function.arguments.as_object() {
                            if let Some(path) = obj.get("path").and_then(|v| v.as_str()) {
                                path.to_string()
                            } else {
                                tool_call.function.arguments.to_string()
                            }
                        } else {
                            tool_call.function.arguments.to_string()
                        }
                    }
                    "edit_file" => {
                        if let Some(obj) = tool_call.function.arguments.as_object() {
                            let old_len = obj
                                .get("old_text")
                                .and_then(|v| v.as_str())
                                .map_or(0, |s| s.len());
                            let new_len = obj
                                .get("new_text")
                                .and_then(|v| v.as_str())
                                .map_or(0, |s| s.len());
                            format!(
                                "{} input_len={} output_len={}",
                                file_path.display(),
                                old_len,
                                new_len
                            )
                        } else {
                            tool_call.function.arguments.to_string()
                        }
                    }
                    _ => tool_call.function.arguments.to_string(),
                };
                println!("[tool]: {} {}", tool_call.function.name, msg);
            }
            Ok(MultiTurnStreamItem::FinalResponse(res)) => {
                if is_reasoning && verbose {
                    print!("\n    \x1b[0m");
                }
                let summary = res.response();
                if summary.is_empty() {
                    println!("[{alias}]: Done.");
                } else {
                    println!("[{alias}] Done: {summary}");
                }
            }
            Err(e) => {
                eprintln!("Stream error: {e}");
                break;
            }
            _ => {}
        }
    }

    // Remove context marker lines from the file (they served their purpose).
    remove_context_markers(file_path, alias, &context_markers)?;

    // Show diff if the file changed.
    let content_after = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to re-read: {}", file_path.display()))?;

    if content_before != content_after
        && let Some(cmd) = resolve_diff_tool(diff_tool)
    {
        let label = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        println!("\n--- diff ({label}) ---");
        let diff_output = run_diff(&cmd, label, &content_before, &content_after);
        if !diff_output.is_empty() {
            println!("{diff_output}");
        }
    }
    _reverter.mark_success();
    if personality {
        personality::post_work_personality(alias);
    }

    Ok(all_markers.len())
}

async fn process_scan_and_complete<C>(
    comp_client: &C,
    model_name: &str,
    alias: &str,
    diff_tool: Option<&Vec<String>>,
    pattern: &str,
    verbose: bool,
    personality: bool,
) -> anyhow::Result<usize>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    let files = expand_glob(pattern)?;
    if files.is_empty() {
        anyhow::bail!("No files matched pattern: {pattern}");
    }

    let mut total = 0usize;
    for file_path in &files {
        total += process_file_markers(
            comp_client,
            model_name,
            alias,
            diff_tool,
            file_path,
            verbose,
            personality,
        )
        .await?;
    }

    Ok(total)
}

/// Single-pass completion: scan once, process all markers.
pub async fn cmd_complete(
    config: &Config,
    alias: &str,
    pattern: String,
    verbose: bool,
) -> anyhow::Result<()> {
    let diff_tool = config.diff_tool.as_ref();
    let count = scan_and_complete_dispatch(
        &config.model,
        alias,
        diff_tool,
        &pattern,
        verbose,
        config.personality,
    )
    .await?;

    if count == 0 {
        println!("No '{alias}:' markers found.");
    } else {
        println!("Completed {count} marker(s).");
    }

    Ok(())
}

/// Compute the common ancestor directory of two paths.
fn common_ancestor(a: &std::path::Path, b: &std::path::Path) -> std::path::PathBuf {
    let a_components = a.components();
    let b_components = b.components();
    let mut common = std::path::PathBuf::new();
    for (ca, cb) in a_components.zip(b_components) {
        if ca == cb {
            common.push(ca);
        } else {
            break;
        }
    }
    common
}

/// Compute a lightweight hash of file contents for change detection.
fn content_hash(path: &std::path::Path) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let content = std::fs::read_to_string(path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    Some(hasher.finish())
}

/// Snapshot hashes of all files matching the glob pattern.
fn snapshot_hashes(pattern: &str) -> std::collections::HashMap<std::path::PathBuf, u64> {
    let mut hashes = std::collections::HashMap::new();
    if let Ok(files) = crate::helpers::expand_glob(pattern) {
        for path in files {
            if let Some(h) = content_hash(&path) {
                hashes.insert(path, h);
            }
        }
    }
    hashes
}

/// Check whether any file matching the glob has changed since `prev`.
/// Returns true if at least one file has a different hash or is new.
fn files_changed(pattern: &str, prev: &std::collections::HashMap<std::path::PathBuf, u64>) -> bool {
    if let Ok(files) = crate::helpers::expand_glob(pattern) {
        for path in &files {
            match content_hash(path) {
                Some(h) => match prev.get(path) {
                    Some(&prev_h) if prev_h == h => {}
                    _ => return true,
                },
                None => return true,
            }
        }
        // Also detect files that were removed.
        for prev_path in prev.keys() {
            if !files.contains(prev_path) {
                return true;
            }
        }
    }
    false
}

/// Watch mode: continuously monitor files for new/changed markers.
pub async fn cmd_watch(
    config: &Config,
    alias: &str,
    pattern: String,
    verbose: bool,
) -> anyhow::Result<()> {
    use notify::{Event, RecursiveMode, Watcher, recommended_watcher};
    use std::sync::mpsc;

    // Expand the glob to find actual files, then derive a watch root.
    let mut watch_path = crate::helpers::expand_glob(&pattern)
        .context("Failed to expand glob pattern")?
        .into_iter()
        .fold(None, |acc: Option<std::path::PathBuf>, path| match acc {
            None => Some(path.as_path().to_path_buf()),
            Some(base) => Some(common_ancestor(&base, path.as_path())),
        })
        .unwrap_or_else(|| std::path::Path::new(".").to_path_buf());

    // Ensure we watch a directory, not a file.
    while !watch_path.is_dir() {
        watch_path = watch_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
    }

    println!(
        "Watching {} for '{alias}:' markers (pattern: {pattern})...",
        watch_path.display()
    );
    println!("Press SPACE to stop current work, Ctrl+C to quit.\n");

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = recommended_watcher(tx)?;
    watcher.watch(&watch_path, RecursiveMode::Recursive)?;

    let diff_tool = config.diff_tool.as_ref();

    // Initial scan — always run, then snapshot hashes.
    let _ = scan_and_complete_dispatch(
        &config.model,
        alias,
        diff_tool,
        &pattern,
        verbose,
        config.personality,
    )
    .await;
    let mut prev_hashes = snapshot_hashes(&pattern);

    loop {
        if crate::keyboard::should_stop() {
            crate::keyboard::clear_stop();
            continue;
        }
        if cleanup::is_shutting_down() {
            break;
        }

        match rx.recv() {
            Ok(Ok(_event)) => {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                while rx.try_recv().is_ok() {}

                // Skip processing if no file content has actually changed.
                if !files_changed(&pattern, &prev_hashes) {
                    continue;
                }

                if let Err(e) = scan_and_complete_dispatch(
                    &config.model,
                    alias,
                    diff_tool,
                    &pattern,
                    verbose,
                    config.personality,
                )
                .await
                {
                    eprintln!("Watch error: {e:?}");
                }
                prev_hashes = snapshot_hashes(&pattern);
            }
            Ok(Err(e)) => {
                eprintln!("Watch error: {e}");
            }
            Err(mpsc::RecvError) => {
                break;
            }
        }
    }

    Ok(())
}
