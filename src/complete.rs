use anyhow::Context;
use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::streaming::{StreamedAssistantContent, StreamingPrompt};
use std::io::Write;

use crate::config::{ModelConfig, Provider};
use crate::helpers::{expand_glob, resolve_diff_tool, run_diff};
use crate::markers::MarkerKind;
use crate::state::AppState;
use crate::{cleanup, personality, raii, tools};

#[derive(Default)]
struct ScanOutcome {
    completed_markers: usize,
    answered_questions: usize,
}

impl ScanOutcome {
    fn add(&mut self, other: Self) {
        self.completed_markers += other.completed_markers;
        self.answered_questions += other.answered_questions;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MarkerOutput {
    verbose: bool,
    tool_calls: bool,
    personality: bool,
}

impl MarkerOutput {
    fn for_marker(marker: &crate::markers::FoundMarker, verbose: bool, personality: bool) -> Self {
        if is_question_marker(marker) {
            Self {
                verbose: false,
                tool_calls: false,
                personality: false,
            }
        } else {
            Self {
                verbose,
                tool_calls: true,
                personality,
            }
        }
    }
}

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
            app_state: &AppState,
            cfg: &ModelConfig,
            alias: &str,
            diff_tool: Option<&Vec<String>>,
            pattern: &str,
            verbose: bool,
            personality: bool,
        ) -> anyhow::Result<ScanOutcome> {
            match cfg.provider {
                $(
                    Provider::$variant => {
                        let client = crate::helpers::$fn_name(cfg)
                            .with_context(|| format!("Failed to build {:?} client", Provider::$variant))?;
                        process_scan_and_complete::<$client_type>(
                            app_state,
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

fn display_tool_path(app_state: &AppState, path: &str) -> String {
    match app_state.resolve_path(path) {
        Ok(_) => path.to_string(),
        Err(_) => format!("<denied: {path}>"),
    }
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

The prompt identifies exactly one marker to process. Work only on that marker; \
other markers are handled separately.

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

fn is_question_marker(marker: &crate::markers::FoundMarker) -> bool {
    marker.kind == MarkerKind::Task && marker.query.trim_end().ends_with('?')
}

fn question_allows_dynamic_tools(marker: &crate::markers::FoundMarker) -> bool {
    marker.query.split_whitespace().any(|word| {
        let word = word.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '+');
        word == "+tool" || word == "+tools"
    })
}

fn question_text(marker: &crate::markers::FoundMarker) -> String {
    marker
        .query
        .split_whitespace()
        .filter(|word| *word != "+tool" && *word != "+tools")
        .collect::<Vec<_>>()
        .join(" ")
}

fn make_question_preamble(alias: &str) -> String {
    format!(
        "You answer questions written in '{alias}:' file markers. This is a strictly read-only \
         mode: you have no tools that modify files. Read files only when needed for context. \
         Answer the questions directly and concisely. Do not describe your process, mention \
         tools, add personality, or propose edits."
    )
}

async fn answer_questions<C>(
    app_state: &AppState,
    comp_client: &C,
    model_name: &str,
    alias: &str,
    file_path: &std::path::Path,
    content: &str,
    question_marker: &crate::markers::FoundMarker,
) -> anyhow::Result<bool>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    let prompt = format!(
        "Target file: {}\nFile type: {}\n\nQUESTION at line {}: {}\n\
         Surrounding context:\n{}\n\nAnswer the question directly.",
        file_path.display(),
        file_extension(file_path),
        question_marker.start_line,
        question_text(question_marker),
        surrounding_lines(content, question_marker.start_line, 5),
    );

    let mut agent_builder = comp_client
        .agent(model_name)
        .preamble(&make_question_preamble(alias))
        .tool(tools::ReadFileTool::default())
        .tool(tools::ListFilesTool::default())
        .default_max_turns(30);
    if question_allows_dynamic_tools(question_marker) {
        agent_builder =
            agent_builder.tools(tools::find_dynamic_tools(content, alias, &app_state.path));
    }
    let agent = agent_builder.build();
    let mut stream = agent.stream_prompt(&prompt).await;
    let mut answered = false;

    while let Some(item) = stream.next().await {
        if cleanup::is_shutting_down() || crate::keyboard::should_stop() {
            crate::keyboard::clear_stop();
            return Ok(false);
        }
        match item {
            Ok(MultiTurnStreamItem::FinalResponse(res)) => {
                let answer = res.response();
                if !answer.is_empty() {
                    println!("{answer}");
                }
                answered = true;
            }
            Err(e) => {
                eprintln!("Stream error: {e}");
                break;
            }
            _ => {}
        }
    }

    if answered {
        app_state.remember_answered_question(file_path, question_marker);
    } else {
        return Ok(false);
    }
    Ok(true)
}

/// Remove one completed marker while leaving every other marker untouched.
fn remove_marker(
    file_path: &std::path::Path,
    alias: &str,
    completed: &crate::markers::FoundMarker,
) -> anyhow::Result<bool> {
    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read for cleanup: {}", file_path.display()))?;

    let markers = crate::markers::find_markers(&content, alias);
    let Some(marker) = markers.iter().find(|marker| {
        marker.start_line == completed.start_line
            && marker.end_line == completed.end_line
            && marker.kind == completed.kind
            && marker.query == completed.query
    }) else {
        return Ok(false);
    };

    let lines: Vec<&str> = content.lines().collect();
    let had_trailing_newline = content.ends_with('\n');
    let kept: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(idx, _)| {
            let line = idx + 1;
            line < marker.start_line || line > marker.end_line
        })
        .map(|(_, line)| *line)
        .collect();

    let mut new_content = kept.join("\n");
    if had_trailing_newline && !new_content.ends_with('\n') {
        new_content.push('\n');
    }

    std::fs::write(file_path, &new_content)
        .with_context(|| format!("Failed to write cleaned file: {}", file_path.display()))?;

    Ok(true)
}

fn remove_context_markers(file_path: &std::path::Path, alias: &str) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read for cleanup: {}", file_path.display()))?;
    let markers = crate::markers::find_markers(&content, alias);
    let remove_lines: std::collections::HashSet<_> = markers
        .iter()
        .filter(|marker| marker.kind == MarkerKind::Context)
        .flat_map(|marker| marker.start_line..=marker.end_line)
        .collect();
    if remove_lines.is_empty() {
        return Ok(());
    }

    let had_trailing_newline = content.ends_with('\n');
    let mut new_content = content
        .lines()
        .enumerate()
        .filter(|(idx, _)| !remove_lines.contains(&(idx + 1)))
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n");
    if had_trailing_newline && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    std::fs::write(file_path, new_content)
        .with_context(|| format!("Failed to write cleaned file: {}", file_path.display()))
}

async fn process_file_markers<C>(
    app_state: &AppState,
    comp_client: &C,
    model_name: &str,
    alias: &str,
    diff_tool: Option<&Vec<String>>,
    file_path: &std::path::Path,
    verbose: bool,
    personality: bool,
) -> anyhow::Result<ScanOutcome>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    let content_before = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read: {}", file_path.display()))?;

    let all_markers = crate::markers::find_markers(&content_before, alias);
    if all_markers
        .iter()
        .filter(|m| m.kind != MarkerKind::Context)
        .count()
        == 0
    {
        return Ok(ScanOutcome::default());
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
            return Ok(ScanOutcome::default());
        }
    }

    let Some(task_marker) = all_markers
        .iter()
        .filter(|marker| marker.kind == MarkerKind::Task)
        .find(|marker| {
            !is_question_marker(marker) || !app_state.question_was_answered(file_path, marker)
        })
        .cloned()
    else {
        return Ok(ScanOutcome::default());
    };
    let output = MarkerOutput::for_marker(&task_marker, verbose, personality);

    if is_question_marker(&task_marker) {
        let answered = answer_questions(
            app_state,
            comp_client,
            model_name,
            alias,
            file_path,
            &content_before,
            &task_marker,
        )
        .await?;
        return Ok(ScanOutcome {
            completed_markers: 0,
            answered_questions: usize::from(answered),
        });
    }

    let context_markers: Vec<_> = all_markers
        .iter()
        .filter(|m| m.kind == MarkerKind::Context)
        .collect();

    println!(
        "Found marker in {} (1 task, {} context)",
        file_path.display(),
        context_markers.len(),
    );

    println!(
        "[{alias}]: Task: {} (L{})",
        task_marker.query, task_marker.start_line
    );
    for m in &context_markers {
        println!("[{alias}]: Context: {} (L{})", m.query, m.start_line);
    }

    let file_display = file_path.display().to_string();
    let task_block = format!(
        "TASK at line {}: {alias}: {}\nSurrounding context:\n{}",
        task_marker.start_line,
        task_marker.query,
        surrounding_lines(&content_before, task_marker.start_line, 5),
    );

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
         {}\n\
         {}\n\n\
         Read the file and any other context you need, then replace this task marker \
         with content that is coherent with the rest of the file.\n\
         Do NOT edit or remove the context note lines yourself; they will be cleaned up automatically.",
        file_extension(file_path),
        task_block,
        context_section,
    );

    let preamble = make_preamble(alias);
    let agent_builder = comp_client
        .agent(model_name)
        .preamble(&preamble)
        .tool(tools::ReadFileTool::default())
        .tool(tools::EditFileTool {
            app_state: crate::state::get(),
            target_path: file_display,
            alias: alias.to_string(),
        })
        .tool(tools::SendMessageTool)
        .tool(tools::ListFilesTool::default())
        .tool(tools::WriteFileTool::default())
        .tools(tools::find_dynamic_tools(
            &content_before,
            alias,
            &app_state.path,
        ))
        .default_max_turns(30);

    let agent = agent_builder.build();

    if output.personality {
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
            return Ok(ScanOutcome::default());
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
            ))) if output.verbose => {
                if !is_reasoning {
                    is_reasoning = true;
                    print!("\n    \x1b[90m// thinking...\x1b[0m\n");
                }
                print!("\x1b[90m{}\x1b[0m", reasoning.display_text());
                std::io::stdout().flush().ok();
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::ReasoningDelta { reasoning, .. },
            )) if output.verbose => {
                if !is_reasoning {
                    is_reasoning = true;
                    print!("\n    \x1b[90m// thinking...\x1b[0m\n");
                }
                print!("\x1b[90m{}\x1b[0m", reasoning);
                std::io::stdout().flush().ok();
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(text)))
                if output.verbose =>
            {
                if is_reasoning && output.verbose {
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
                if is_reasoning && output.verbose {
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
                                display_tool_path(crate::state::get(), path)
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
                    "write_file" => {
                        if let Some(obj) = tool_call.function.arguments.as_object() {
                            if let Some(path) = obj.get("path").and_then(|v| v.as_str()) {
                                display_tool_path(crate::state::get(), path)
                            } else {
                                "???".to_string()
                            }
                        } else {
                            "???".to_string()
                        }
                    }
                    "send_message" => continue,
                    _ => tool_call.function.arguments.to_string(),
                };
                if output.tool_calls {
                    println!("[tool]: {} {}", tool_call.function.name, msg);
                }
            }
            Ok(MultiTurnStreamItem::FinalResponse(res)) => {
                if is_reasoning && output.verbose {
                    print!("\n    \x1b[0m");
                }
                let summary = res.response();
                if summary.is_empty() {
                    println!("[{alias}]: Done.");
                } else {
                    println!("[{alias}]: Done: {summary}");
                }
            }
            Err(e) => {
                eprintln!("Stream error: {e}");
                break;
            }
            _ => {}
        }
    }

    remove_marker(file_path, alias, &task_marker)?;

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
    if output.personality {
        personality::post_work_personality(alias);
    }

    Ok(ScanOutcome {
        completed_markers: 1,
        answered_questions: 0,
    })
}

async fn process_scan_and_complete<C>(
    app_state: &AppState,
    comp_client: &C,
    model_name: &str,
    alias: &str,
    diff_tool: Option<&Vec<String>>,
    pattern: &str,
    verbose: bool,
    personality: bool,
) -> anyhow::Result<ScanOutcome>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    let files = expand_glob(app_state, pattern)?;
    if files.is_empty() {
        anyhow::bail!("No files matched pattern: {pattern}");
    }

    let mut outcome = ScanOutcome::default();
    for file_path in &files {
        let mut file_outcome = ScanOutcome::default();
        loop {
            let processed = process_file_markers(
                app_state,
                comp_client,
                model_name,
                alias,
                diff_tool,
                file_path,
                verbose,
                personality,
            )
            .await?;
            if processed.completed_markers == 0 && processed.answered_questions == 0 {
                break;
            }
            file_outcome.add(processed);
        }
        if file_outcome.completed_markers > 0 {
            remove_context_markers(file_path, alias)?;
        }
        outcome.add(file_outcome);
    }

    Ok(outcome)
}

/// Single-pass completion: scan once, process all markers.
pub async fn cmd_complete(
    app_state: &AppState,
    alias: &str,
    pattern: String,
    verbose: bool,
) -> anyhow::Result<()> {
    let config = &app_state.config;
    let diff_tool = config.diff_tool.as_ref();
    let outcome = scan_and_complete_dispatch(
        app_state,
        &config.model,
        alias,
        diff_tool,
        &pattern,
        verbose,
        config.personality,
    )
    .await?;

    if outcome.completed_markers == 0 && outcome.answered_questions == 0 {
        println!("No '{alias}:' markers found.");
    } else if outcome.completed_markers > 0 {
        println!("Completed {} marker(s).", outcome.completed_markers);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_tool_path_denies_paths_outside_watched_directory() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let state = AppState::new(dir.path().to_path_buf(), crate::config::Config::default())?;

        assert_eq!(
            display_tool_path(&state, "../outside.txt"),
            "<denied: ../outside.txt>"
        );
        Ok(())
    }

    #[test]
    fn display_tool_path_allows_absolute_paths_inside_watched_directory() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let state = AppState::new(dir.path().to_path_buf(), crate::config::Config::default())?;
        let path = state.path.join("inside.txt");
        let path = path.to_string_lossy();

        assert_eq!(display_tool_path(&state, &path), path);
        Ok(())
    }

    #[test]
    fn question_markers_end_with_question_mark() {
        let markers = crate::markers::find_markers(
            "rik: why is this slow?   \nrik: make this faster\nrik: /context?/",
            "rik",
        );

        let questions: Vec<_> = markers
            .iter()
            .filter(|marker| is_question_marker(marker))
            .collect();

        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].query, "why is this slow?");
    }

    #[test]
    fn questions_require_explicit_dynamic_tool_authorization() {
        let markers =
            crate::markers::find_markers("rik: why?\nrik: +tool why?\nrik: why +tools ?", "rik");

        assert!(!question_allows_dynamic_tools(&markers[0]));
        assert!(question_allows_dynamic_tools(&markers[1]));
        assert!(question_allows_dynamic_tools(&markers[2]));
        assert_eq!(question_text(&markers[1]), "why?");
        assert_eq!(question_text(&markers[2]), "why ?");
    }

    #[test]
    fn question_marker_output_silences_tools_verbose_and_personality() {
        let question = crate::markers::find_markers("rik: why?", "rik")
            .into_iter()
            .next()
            .unwrap();
        let edit = crate::markers::find_markers("rik: do it", "rik")
            .into_iter()
            .next()
            .unwrap();

        assert_eq!(
            MarkerOutput::for_marker(&question, true, true),
            MarkerOutput {
                verbose: false,
                tool_calls: false,
                personality: false,
            }
        );
        assert_eq!(
            MarkerOutput::for_marker(&edit, true, true),
            MarkerOutput {
                verbose: true,
                tool_calls: true,
                personality: true,
            }
        );
    }

    #[test]
    fn answered_question_memory_filters_exact_marker_identity() {
        let file = std::path::Path::new("/tmp/question-memory-test.rs");
        let markers = crate::markers::find_markers("rik: why?\nrik: why?", "rik");
        let dir = tempfile::tempdir().unwrap();
        let state =
            AppState::new(dir.path().to_path_buf(), crate::config::Config::default()).unwrap();

        state.remember_answered_question(file, &markers[0]);

        assert!(state.question_was_answered(file, &markers[0]));
        assert!(!state.question_was_answered(file, &markers[1]));
    }

    #[test]
    fn completed_marker_cleanup_leaves_question_and_later_markers() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.rs");
        std::fs::write(
            &file,
            "rik: first task\nrik: why?\nrik: second task\ncontent\n",
        )?;
        let markers = crate::markers::find_markers(&std::fs::read_to_string(&file)?, "rik");

        assert!(remove_marker(&file, "rik", &markers[0])?);
        assert_eq!(
            std::fs::read_to_string(&file)?,
            "rik: why?\nrik: second task\ncontent\n"
        );
        Ok(())
    }
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
fn snapshot_hashes(
    app_state: &AppState,
    pattern: &str,
) -> std::collections::HashMap<std::path::PathBuf, u64> {
    let mut hashes = std::collections::HashMap::new();
    if let Ok(files) = crate::helpers::expand_glob(app_state, pattern) {
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
fn files_changed(
    app_state: &AppState,
    pattern: &str,
    prev: &std::collections::HashMap<std::path::PathBuf, u64>,
) -> bool {
    if let Ok(files) = crate::helpers::expand_glob(app_state, pattern) {
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
    app_state: &AppState,
    alias: &str,
    pattern: String,
    verbose: bool,
) -> anyhow::Result<()> {
    use notify::{Event, RecursiveMode, Watcher, recommended_watcher};
    use std::sync::mpsc;

    let watch_path = &app_state.path;

    println!(
        "Watching {} for '{alias}:' markers (pattern: {pattern})...",
        watch_path.display()
    );
    println!("Press SPACE to stop current work, Ctrl+C to quit.\n");

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = recommended_watcher(tx)?;
    watcher.watch(watch_path, RecursiveMode::Recursive)?;

    let config = &app_state.config;
    let diff_tool = config.diff_tool.as_ref();

    // Initial scan — always run, then snapshot hashes.
    let _ = scan_and_complete_dispatch(
        app_state,
        &config.model,
        alias,
        diff_tool,
        &pattern,
        verbose,
        config.personality,
    )
    .await;
    let mut prev_hashes = snapshot_hashes(app_state, &pattern);

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
                if !files_changed(app_state, &pattern, &prev_hashes) {
                    continue;
                }

                if let Err(e) = scan_and_complete_dispatch(
                    app_state,
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
                prev_hashes = snapshot_hashes(app_state, &pattern);
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
