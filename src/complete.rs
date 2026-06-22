use anyhow::Context;
use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::completion::message::ToolResultContent;
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingPrompt};
use std::collections::HashMap;
use std::io::Write;

use crate::config::{EditionConstraints, ModelConfig, Provider};
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

#[derive(Clone, Copy)]
struct ProcessingOptions<'a> {
    alias: &'a str,
    diff_tool: Option<&'a Vec<String>>,
    verbose: bool,
    personality: bool,
    no_ignore: bool,
    system_prompt: Option<&'a str>,
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
            no_ignore: bool,
            system_prompt: Option<&str>,
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
                            pattern,
                            ProcessingOptions {
                                alias,
                                diff_tool,
                                verbose,
                                personality,
                                no_ignore,
                                system_prompt,
                            },
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
        Ok(path) => path
            .strip_prefix(&app_state.path)
            .map(|relative| {
                if relative.as_os_str().is_empty() {
                    ".".to_string()
                } else {
                    relative.display().to_string()
                }
            })
            .unwrap_or_else(|_| format!("<denied: {}>", path.display())),
        Err(_) => format!("<denied: {path}>"),
    }
}

fn display_read_file_call(
    app_state: &AppState,
    args: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let path = args
        .get("path")
        .and_then(|value| value.as_str())
        .map_or_else(
            || "???".to_string(),
            |path| display_tool_path(app_state, path),
        );
    let offset = args
        .get("offset")
        .and_then(|value| value.as_u64())
        .unwrap_or(1);
    let lines = match args.get("limit").and_then(|value| value.as_u64()) {
        Some(limit) => format!(
            "{offset}-{}",
            offset.saturating_add(limit).saturating_sub(1)
        ),
        None if offset == 1 => "all".to_string(),
        None => format!("{offset}-"),
    };
    format!("{path} lines={lines}")
}

fn tool_result_text(tool_result: &rig::completion::message::ToolResult) -> String {
    tool_result
        .content
        .clone()
        .into_iter()
        .filter_map(|content| match content {
            ToolResultContent::Text(text) => Some(text.text),
            ToolResultContent::Image(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn display_tool_error(result: &str) -> &str {
    result.strip_prefix("ToolCallError: ").unwrap_or(result)
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
fn append_system_prompt(mut preamble: String, system_prompt: Option<&str>) -> String {
    if let Some(system_prompt) = system_prompt {
        preamble.push_str(
            "\n\nAdditional overarching instructions. Follow them where they do not conflict \
             with Rik's operational rules above:\n",
        );
        preamble.push_str(system_prompt);
    }
    preamble
}

fn make_preamble(alias: &str, tool_inject: &str, system_prompt: Option<&str>) -> String {
    append_system_prompt(format!("\
You are an in-place editor. A file contains '{alias}: <instruction>' markers that \
must be replaced with real content. The file is a working document (code, prose, \
config, etc.) and your edits must keep it coherent and correct.

The prompt identifies exactly one marker to process. Work only on that marker; \
other markers are handled separately.

Tools:
- read_file: read other files for context (types, imports, conventions).
- edit_file: replace exact text in the target file. old_text must be unique.
- list_files: discover files in the project.
{tool_inject}

Rules:
- Always use absolute paths when calling file tools. Relative paths are only \
  used in Rik's human-facing console output.
- Study the surrounding lines BEFORE editing. Your replacement must fit the \
  existing style, indentation, language, and intent of the file.
- If the file is code, respect existing imports, types, and variable names. \
  Add needed imports only if you can verify they are missing.
- If you are unsure about conventions, read nearby files for reference.
- You may make MULTIPLE edit_file calls if the change requires touching more \
  than one spot (e.g. adding an import AND inserting code).
- A task is incomplete until you make a substantive edit to the target file. \
  Reading context and returning a summary without editing is not completion.
- Each edit_file call must have a unique old_text match.
- Do NOT add comments explaining what you did. Just make the edit.
- Do NOT echo back the file contents. The edit_file call IS your output.
- After editing, provide a SHORT summary of what you changed (under 250 characters). \
  A diff of your changes will be shown to the user separately, so focus on intent, not line-by-line description."),
        system_prompt,
    )
}

fn is_question_marker(marker: &crate::markers::FoundMarker) -> bool {
    marker.kind == MarkerKind::Task
        && (marker.query.trim_end().ends_with('?')
            || marker.query.trim_start().starts_with('?')
            || marker.prefix.contains("?"))
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

fn marker_replacement_instruction(
    marker: &crate::markers::FoundMarker,
    edition_constraints: EditionConstraints,
) -> String {
    if edition_constraints == EditionConstraints::VicinityBefore {
        if marker.start_line < marker.end_line {
            format!(
                "Replace the entire multiline marker span from line {} through line {}, including \
                 the opening marker, enclosed text, and closing delimiter. Do not replace only the \
                 opening marker line.",
                marker.start_line, marker.end_line
            )
        } else {
            format!("Replace the task marker on line {}.", marker.start_line)
        }
    } else if marker.start_line < marker.end_line {
        format!(
            "The multiline marker spans lines {}-{} and anchors the requested edit. \
             Edit the target code directly; you may update the enclosed body or replace the full \
             marker span when that is the clearest change.",
            marker.start_line, marker.end_line
        )
    } else {
        format!(
            "The task marker on line {} anchors the requested edit. \
             Edit the target code directly, including nearby existing lines when the task asks for \
             a modification; do not treat the marker as the only insertion point.",
            marker.start_line
        )
    }
}

fn make_question_preamble(alias: &str, system_prompt: Option<&str>) -> String {
    append_system_prompt(
        format!(
            "You answer questions written in '{alias}:' file markers. This is a strictly read-only \
         mode: you have no tools that modify files. Read files only when needed for context. \
         Answer the questions directly and concisely. Do not describe your process, mention \
         tools, add personality, or propose edits."
        ),
        system_prompt,
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
    system_prompt: Option<&str>,
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
        .preamble(&make_question_preamble(alias, system_prompt))
        .tool(tools::ReadFileTool::default())
        .tool(tools::ListFilesTool::default())
        .default_max_turns(30);
    let (_, tools) = tools::find_dynamic_tools(content, alias, &app_state.path);
    if question_allows_dynamic_tools(question_marker) {
        agent_builder = agent_builder.tools(tools);
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
                    println!("\n\n== ANSWER START ==\n");
                    println!("{answer}");
                    println!("\n== ANSWER END ==");
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
///
/// For multiline markers, remove only the opening and closing delimiter lines.
/// The agent may have edited the enclosed content without replacing the whole
/// span, so deleting the interior here would discard a successful edit.
fn remove_marker(
    file_path: &std::path::Path,
    alias: &str,
    completed: &crate::markers::FoundMarker,
) -> anyhow::Result<bool> {
    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read for cleanup: {}", file_path.display()))?;

    let markers = crate::markers::find_markers(&content, alias);
    let marker = markers.iter().find(|marker| {
        marker.start_line == completed.start_line
            && marker.end_line == completed.end_line
            && marker.kind == completed.kind
            && marker.query == completed.query
    });
    let marker = marker.or_else(|| {
        markers.iter().find(|marker| {
            completed.start_line < completed.end_line
                && marker.start_line < marker.end_line
                && marker.start_line == completed.start_line
                && marker.kind == completed.kind
                && marker.prefix == completed.prefix
        })
    });
    let Some(marker) = marker else {
        return Ok(false);
    };

    let lines: Vec<&str> = content.lines().collect();
    let had_trailing_newline = content.ends_with('\n');
    let kept: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(idx, _)| {
            let line = idx + 1;
            if marker.start_line == marker.end_line {
                line != marker.start_line
            } else {
                line != marker.start_line && line != marker.end_line
            }
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

/// Clean up a completed marker only after the agent changed the target file.
fn remove_marker_after_change(
    file_path: &std::path::Path,
    alias: &str,
    completed: &crate::markers::FoundMarker,
    content_before: &str,
) -> anyhow::Result<bool> {
    let content_after = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to verify edit: {}", file_path.display()))?;
    if content_after == content_before {
        return Ok(false);
    }

    remove_marker(file_path, alias, completed)?;
    Ok(true)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChangedBlock {
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
}

fn line_chunks(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    content.split_inclusive('\n').collect()
}

fn changed_blocks(original: &str, produced: &str) -> Vec<ChangedBlock> {
    let old_lines = line_chunks(original);
    let new_lines = line_chunks(produced);
    if old_lines == new_lines {
        return Vec::new();
    }

    if old_lines.len().saturating_mul(new_lines.len()) > 4_000_000 {
        return vec![ChangedBlock {
            old_start: 0,
            old_end: old_lines.len(),
            new_start: 0,
            new_end: new_lines.len(),
        }];
    }

    let width = new_lines.len() + 1;
    let mut lcs = vec![0usize; (old_lines.len() + 1) * width];
    for old_idx in (0..old_lines.len()).rev() {
        for new_idx in (0..new_lines.len()).rev() {
            let pos = old_idx * width + new_idx;
            lcs[pos] = if old_lines[old_idx] == new_lines[new_idx] {
                lcs[(old_idx + 1) * width + new_idx + 1] + 1
            } else {
                lcs[(old_idx + 1) * width + new_idx].max(lcs[old_idx * width + new_idx + 1])
            };
        }
    }

    let mut blocks = Vec::new();
    let mut old_idx = 0;
    let mut new_idx = 0;
    let mut current: Option<ChangedBlock> = None;
    while old_idx < old_lines.len() || new_idx < new_lines.len() {
        if old_idx < old_lines.len()
            && new_idx < new_lines.len()
            && old_lines[old_idx] == new_lines[new_idx]
        {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            old_idx += 1;
            new_idx += 1;
        } else {
            let block = current.get_or_insert(ChangedBlock {
                old_start: old_idx,
                old_end: old_idx,
                new_start: new_idx,
                new_end: new_idx,
            });
            if new_idx == new_lines.len()
                || (old_idx < old_lines.len()
                    && lcs[(old_idx + 1) * width + new_idx] >= lcs[old_idx * width + new_idx + 1])
            {
                old_idx += 1;
                block.old_end = old_idx;
            } else {
                new_idx += 1;
                block.new_end = new_idx;
            }
        }
    }
    if let Some(block) = current {
        blocks.push(block);
    }
    blocks
}

fn line_range_touches_marker_vicinity(
    start: usize,
    end: usize,
    marker_spans: &[(usize, usize)],
) -> bool {
    if marker_spans.is_empty() {
        return true;
    }
    let start_line = start + 1;
    let end_line = end.max(start + 1);
    marker_spans.iter().any(|&(marker_start, marker_end)| {
        let vicinity_start = marker_start.saturating_sub(tools::edit_file::MARKER_RADIUS);
        let vicinity_end = marker_end + tools::edit_file::MARKER_RADIUS;
        start_line <= vicinity_end && end_line >= vicinity_start
    })
}

fn insertion_touches_marker(block: &ChangedBlock, marker_spans: &[(usize, usize)]) -> bool {
    if block.old_start != block.old_end || block.new_start == block.new_end {
        return false;
    }

    let insertion_line = block.old_start + 1;
    marker_spans.iter().any(|&(marker_start, marker_end)| {
        insertion_line >= marker_start && insertion_line <= marker_end + 1
    })
}

fn block_allowed(block: &ChangedBlock, marker_spans: &[(usize, usize)]) -> bool {
    if insertion_touches_marker(block, marker_spans) {
        return false;
    }

    line_range_touches_marker_vicinity(block.old_start, block.old_end, marker_spans)
        || line_range_touches_marker_vicinity(block.new_start, block.new_end, marker_spans)
}

fn apply_vicinity_after_constraints(
    file_path: &std::path::Path,
    alias: &str,
    original: &str,
) -> anyhow::Result<usize> {
    let produced = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to re-read: {}", file_path.display()))?;
    let blocks = changed_blocks(original, &produced);
    if blocks.is_empty() {
        return Ok(0);
    }

    let marker_spans = tools::edit_file::editable_marker_spans(original, alias);
    let rejected = blocks
        .iter()
        .filter(|block| !block_allowed(block, &marker_spans))
        .count();
    if rejected == 0 {
        return Ok(0);
    }

    let old_lines = line_chunks(original);
    let new_lines = line_chunks(&produced);
    let mut reconciled = String::new();
    let mut old_cursor = 0;
    for block in blocks {
        reconciled.push_str(&old_lines[old_cursor..block.old_start].concat());
        if block_allowed(&block, &marker_spans) {
            reconciled.push_str(&new_lines[block.new_start..block.new_end].concat());
        } else {
            reconciled.push_str(&old_lines[block.old_start..block.old_end].concat());
        }
        old_cursor = block.old_end;
    }
    reconciled.push_str(&old_lines[old_cursor..].concat());
    std::fs::write(file_path, reconciled)
        .with_context(|| format!("Failed to write reconciled file: {}", file_path.display()))?;
    Ok(rejected)
}

fn remove_context_markers(file_path: &std::path::Path, alias: &str) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read for cleanup: {}", file_path.display()))?;
    let markers = crate::markers::find_markers(&content, alias);
    let remove_lines: std::collections::HashSet<_> = markers
        .iter()
        .filter(|marker| {
            marker.kind == MarkerKind::Context
                && !crate::markers::is_stopped(&content, alias, marker)
        })
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
    file_path: &std::path::Path,
    options: ProcessingOptions<'_>,
) -> anyhow::Result<ScanOutcome>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    let ProcessingOptions {
        alias,
        diff_tool,
        verbose,
        personality,
        system_prompt,
        ..
    } = options;
    let content_before = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read: {}", file_path.display()))?;

    let all_markers = crate::markers::find_markers(&content_before, alias);
    if all_markers
        .iter()
        .filter(|marker| {
            marker.kind == MarkerKind::Task
                && !crate::markers::is_stopped(&content_before, alias, marker)
        })
        .count()
        == 0
    {
        return Ok(ScanOutcome::default());
    }

    let Some(task_marker) = all_markers
        .iter()
        .filter(|marker| marker.kind == MarkerKind::Task)
        .find(|marker| {
            !crate::markers::is_stopped(&content_before, alias, marker)
                && (!is_question_marker(marker)
                    || !app_state.question_was_answered(file_path, marker))
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
            system_prompt,
        )
        .await?;
        return Ok(ScanOutcome {
            completed_markers: 0,
            answered_questions: usize::from(answered),
        });
    }

    let context_markers: Vec<_> = all_markers
        .iter()
        .filter(|marker| {
            marker.kind == MarkerKind::Context
                && !crate::markers::is_stopped(&content_before, alias, marker)
        })
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
        "TASK at lines {}-{}: {alias}: {}\n{}\nSurrounding context:\n{}",
        task_marker.start_line,
        task_marker.end_line,
        task_marker.query,
        marker_replacement_instruction(&task_marker, app_state.config.edition_constraints),
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
    let (tool_hash, dynamic_tools) =
        tools::find_dynamic_tools(&content_before, alias, &app_state.path);

    let tool_inject = tool_hash
        .iter()
        .map(|(k, (_, d))| format!("- {k}: {d}"))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Target file: {file_display}\n\
         File type: {}\n\
         {}\n\
         {}\n\n\
         Read the file and any other context you need, then perform the specified replacement \
         with content that is coherent with the rest of the file.\n\
         Do NOT edit or remove the context note lines yourself; they will be cleaned up automatically.",
        file_extension(file_path),
        task_block,
        context_section,
    );

    let preamble = make_preamble(alias, &tool_inject, system_prompt);
    let read_history = std::sync::Arc::new(tools::ReadFileHistory::default());
    let agent_builder = comp_client
        .agent(model_name)
        .preamble(&preamble)
        .tool(tools::ReadFileTool::with_history(
            crate::state::get(),
            read_history.clone(),
        ))
        .tool(tools::EditFileTool {
            app_state: crate::state::get(),
            target_path: file_display,
            alias: alias.to_string(),
            read_history,
        })
        .tool(tools::SendMessageTool)
        .tool(tools::ListFilesTool::default())
        .tool(tools::WriteFileTool::default())
        .tools(dynamic_tools)
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
    let mut pending_edit_diffs = HashMap::new();

    while let Some(item) = stream.next().await {
        if cleanup::is_shutting_down() || crate::keyboard::should_stop() {
            if crate::keyboard::is_soft_stop() {
                _reverter.mark_success();
            }
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
                internal_call_id,
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
                                parts.push(format!(
                                    "path={}",
                                    display_tool_path(crate::state::get(), path)
                                ));
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
                            display_read_file_call(crate::state::get(), obj)
                        } else {
                            tool_call.function.arguments.to_string()
                        }
                    }
                    "edit_file" => {
                        if let Some(obj) = tool_call.function.arguments.as_object() {
                            let old_text =
                                obj.get("old_text").and_then(|v| v.as_str()).unwrap_or("");
                            let new_text =
                                obj.get("new_text").and_then(|v| v.as_str()).unwrap_or("");
                            let path = display_tool_path(
                                crate::state::get(),
                                file_path.to_string_lossy().as_ref(),
                            );
                            let label = file_path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("file")
                                .to_string();
                            pending_edit_diffs.insert(
                                internal_call_id,
                                (
                                    path.clone(),
                                    label,
                                    old_text.to_string(),
                                    new_text.to_string(),
                                ),
                            );
                            path
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
                    dynamic if tool_hash.contains_key(dynamic) => {
                        let (cmd, _) = tool_hash.get(dynamic).cloned().unwrap_or_default();
                        let params = if let Some(obj) = tool_call.function.arguments.as_object() {
                            obj.iter()
                                .map(|(k, v)| format!("{k}={v}"))
                                .collect::<Vec<_>>()
                                .join(" ")
                        } else if let Some(list) = tool_call.function.arguments.as_array() {
                            list.clone()
                                .into_iter()
                                .map(|v| format!("{v}"))
                                .collect::<Vec<_>>()
                                .join(" ")
                        } else {
                            tool_call.function.arguments.to_string()
                        };
                        if params.is_empty() {
                            format!("\"{cmd}\"")
                        } else {
                            format!("\"{cmd}\" params: {params}")
                        }
                    }
                    _ => tool_call.function.arguments.to_string(),
                };
                if output.tool_calls {
                    println!("[tool]: {} {}", tool_call.function.name, msg);
                }
            }
            Ok(MultiTurnStreamItem::StreamUserItem(StreamedUserContent::ToolResult {
                tool_result,
                internal_call_id,
            })) => {
                if let Some((path, label, old_text, new_text)) =
                    pending_edit_diffs.remove(&internal_call_id)
                {
                    let result = tool_result_text(&tool_result);
                    if result.starts_with("[edit_file]") {
                        if output.tool_calls
                            && app_state.config.edition_constraints
                                != EditionConstraints::VicinityAfter
                            && let Some(cmd) = resolve_diff_tool(diff_tool)
                        {
                            println!("--- diff ({path}) ---");
                            let diff_output = run_diff(&cmd, &label, &old_text, &new_text);
                            if !diff_output.is_empty() {
                                println!("{diff_output}");
                            }
                        }
                    } else if output.tool_calls {
                        println!("[tool]: edit_file error: {}", display_tool_error(&result));
                    }
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

                // Revert on error
                return Ok(ScanOutcome::default());
            }
            _ => {}
        }
    }

    if app_state.config.edition_constraints == EditionConstraints::VicinityAfter {
        let rejected = apply_vicinity_after_constraints(file_path, alias, &content_before)?;
        if rejected > 0 {
            println!("[{alias}]: edition-constraints restored {rejected} stray change(s).");
        }
    }

    if !remove_marker_after_change(file_path, alias, &task_marker, &content_before)? {
        _reverter.mark_success();
        anyhow::bail!(
            "Task marker at {}:{} was left unchanged because the agent made no edit",
            file_path.display(),
            task_marker.start_line,
        );
    }

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
    pattern: &str,
    options: ProcessingOptions<'_>,
) -> anyhow::Result<ScanOutcome>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    let alias = options.alias;
    let files = expand_glob(app_state, pattern, options.no_ignore)?;
    if files.is_empty() {
        anyhow::bail!("No files matched pattern: {pattern}");
    }

    let mut outcome = ScanOutcome::default();
    for file_path in &files {
        let mut file_outcome = ScanOutcome::default();
        loop {
            let processed =
                process_file_markers(app_state, comp_client, model_name, file_path, options)
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
    no_ignore: bool,
    system_prompt: Option<&str>,
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
        no_ignore,
        system_prompt,
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
    fn display_tool_path_makes_paths_inside_watched_directory_relative() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let state = AppState::new(dir.path().to_path_buf(), crate::config::Config::default())?;
        let path = state.path.join("inside.txt");
        let path = path.to_string_lossy();

        assert_eq!(display_tool_path(&state, &path), "inside.txt");
        Ok(())
    }

    #[test]
    fn display_read_file_call_includes_requested_lines() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let state = AppState::new(dir.path().to_path_buf(), crate::config::Config::default())?;
        let args = serde_json::json!({
            "path": state.path.join("src/main.rs"),
            "offset": 20,
            "limit": 11,
        });

        assert_eq!(
            display_read_file_call(&state, args.as_object().unwrap()),
            "src/main.rs lines=20-30"
        );
        Ok(())
    }

    #[test]
    fn display_tool_error_removes_rig_prefix() {
        assert_eq!(
            display_tool_error("ToolCallError: old_text not found"),
            "old_text not found"
        );
        assert_eq!(display_tool_error("plain error"), "plain error");
    }

    #[test]
    fn system_prompt_is_appended_to_task_and_question_preambles() {
        let instruction = "Make every response a joke.";

        let task = make_preamble("rik", "", Some(instruction));
        let question = make_question_preamble("rik", Some(instruction));

        assert!(task.ends_with(instruction));
        assert!(question.ends_with(instruction));
    }

    #[test]
    fn absent_system_prompt_does_not_add_overarching_instructions() {
        let task = make_preamble("rik", "", None);
        let question = make_question_preamble("rik", None);

        assert!(!task.contains("Additional overarching instructions"));
        assert!(!question.contains("Additional overarching instructions"));
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
    fn multiline_replacement_instruction_requires_whole_span() {
        let marker = crate::markers::find_markers(
            "// rik: [ uppercase this text\nA lone oak stands.\n// ]",
            "rik",
        )
        .remove(0);

        assert_eq!(marker.start_line, 1);
        assert_eq!(marker.end_line, 3);
        assert_eq!(
            marker_replacement_instruction(&marker, EditionConstraints::VicinityBefore),
            "Replace the entire multiline marker span from line 1 through line 3, including the \
             opening marker, enclosed text, and closing delimiter. Do not replace only the \
             opening marker line."
        );
    }

    #[test]
    fn default_marker_instruction_anchors_without_forcing_insertion() {
        let marker = crate::markers::find_markers("let x = 1;\nrik: make x two", "rik").remove(0);

        let instruction =
            marker_replacement_instruction(&marker, EditionConstraints::VicinityAfter);

        assert!(instruction.contains("anchors the requested edit"));
        assert!(instruction.contains("Edit the target code directly"));
        assert!(instruction.contains("do not treat the marker as the only insertion point"));
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

    #[test]
    fn unchanged_task_is_not_cleaned_up() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.rs");
        let before = "before\nrik: implement this\nafter\n";
        std::fs::write(&file, before)?;
        let marker = crate::markers::find_markers(before, "rik").remove(0);

        assert!(!remove_marker_after_change(&file, "rik", &marker, before)?);
        assert_eq!(std::fs::read_to_string(&file)?, before);
        Ok(())
    }

    #[test]
    fn changed_task_is_cleaned_up() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.rs");
        let before = "before\nrik: implement this\nafter\n";
        std::fs::write(&file, before)?;
        let marker = crate::markers::find_markers(before, "rik").remove(0);
        std::fs::write(&file, "before\nrik: implement this\nimplemented\nafter\n")?;

        assert!(remove_marker_after_change(&file, "rik", &marker, before)?);
        assert_eq!(
            std::fs::read_to_string(&file)?,
            "before\nimplemented\nafter\n"
        );
        Ok(())
    }

    #[test]
    fn changed_multiline_task_cleanup_preserves_edited_body() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.rs");
        let before = "before\nrik: ( uppercase this\nbody\n)\nafter\n";
        std::fs::write(&file, before)?;
        let marker = crate::markers::find_markers(before, "rik").remove(0);
        std::fs::write(&file, "before\nrik: ( uppercase this\nBODY\n)\nafter\n")?;

        assert!(remove_marker_after_change(&file, "rik", &marker, before)?);
        assert_eq!(std::fs::read_to_string(&file)?, "before\nBODY\nafter\n");
        Ok(())
    }

    #[test]
    fn completed_marker_cleanup_preserves_body_with_decorated_multiline_closer()
    -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.rs");
        std::fs::write(&file, "before\n// rik: [[\nwork\n// ]]\nafter\n")?;
        let markers = crate::markers::find_markers(&std::fs::read_to_string(&file)?, "rik");

        assert!(remove_marker(&file, "rik", &markers[0])?);
        assert_eq!(std::fs::read_to_string(&file)?, "before\nwork\nafter\n");
        Ok(())
    }

    #[test]
    fn completed_marker_cleanup_preserves_inline_instruction_multiline_body() -> anyhow::Result<()>
    {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        std::fs::write(
            &file,
            "before\nrik: ( uppercase this\nand entertain ourselves.\n)\nafter\n",
        )?;
        let markers = crate::markers::find_markers(&std::fs::read_to_string(&file)?, "rik");

        assert_eq!(markers[0].query, "uppercase this\nand entertain ourselves.");
        assert!(remove_marker(&file, "rik", &markers[0])?);
        assert_eq!(
            std::fs::read_to_string(&file)?,
            "before\nand entertain ourselves.\nafter\n"
        );
        Ok(())
    }

    #[test]
    fn completed_marker_cleanup_preserves_inline_block_after_body_was_edited() -> anyhow::Result<()>
    {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        let before = "before\nrik: ( uppercase this\nand entertain ourselves.\n)\nafter\n";
        std::fs::write(&file, before)?;
        let completed = crate::markers::find_markers(before, "rik").remove(0);

        std::fs::write(
            &file,
            "before\nrik: ( uppercase this\nAND ENTERTAIN OURSELVES.\n)\nafter\n",
        )?;

        assert!(remove_marker(&file, "rik", &completed)?);
        assert_eq!(
            std::fs::read_to_string(&file)?,
            "before\nAND ENTERTAIN OURSELVES.\nafter\n"
        );
        Ok(())
    }

    #[test]
    fn completed_marker_cleanup_preserves_inline_block_after_body_grows() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        let before = "before\nrik: ( uppercase this\nand entertain ourselves.\n)\nafter\n";
        std::fs::write(&file, before)?;
        let completed = crate::markers::find_markers(before, "rik").remove(0);

        std::fs::write(
            &file,
            "before\nrik: ( uppercase this\nAND ENTERTAIN\nOURSELVES.\n)\nafter\n",
        )?;

        assert!(remove_marker(&file, "rik", &completed)?);
        assert_eq!(
            std::fs::read_to_string(&file)?,
            "before\nAND ENTERTAIN\nOURSELVES.\nafter\n"
        );
        Ok(())
    }

    #[test]
    fn completed_marker_cleanup_after_edit_preserves_later_marker() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        let before = "before\nrik: ( uppercase this\nbody\n)\nmiddle\nrik: later task\nafter\n";
        std::fs::write(&file, before)?;
        let completed = crate::markers::find_markers(before, "rik").remove(0);

        std::fs::write(
            &file,
            "before\nrik: ( uppercase this\nEDITED\nBODY\n)\nmiddle\nrik: later task\nafter\n",
        )?;

        assert!(remove_marker(&file, "rik", &completed)?);
        assert_eq!(
            std::fs::read_to_string(&file)?,
            "before\nEDITED\nBODY\nmiddle\nrik: later task\nafter\n"
        );
        Ok(())
    }

    #[test]
    fn completed_marker_cleanup_does_not_remove_replacement_single_line_marker()
    -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        let before = "before\nrik: original task\nafter\n";
        std::fs::write(&file, before)?;
        let completed = crate::markers::find_markers(before, "rik").remove(0);

        std::fs::write(&file, "before\nrik: replacement task\nafter\n")?;

        assert!(!remove_marker(&file, "rik", &completed)?);
        assert_eq!(
            std::fs::read_to_string(&file)?,
            "before\nrik: replacement task\nafter\n"
        );
        Ok(())
    }

    #[test]
    fn vicinity_after_restores_stray_change_and_keeps_marker_change() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        let before_lines = std::iter::once("stray original".to_string())
            .chain((2..=25).map(|line| format!("line {line}")))
            .chain(["rik: task".to_string(), "near original".to_string()])
            .collect::<Vec<_>>();
        let produced_lines = std::iter::once("stray changed".to_string())
            .chain((2..=25).map(|line| format!("line {line}")))
            .chain(["rik: task".to_string(), "near changed".to_string()])
            .collect::<Vec<_>>();
        let before = format!("{}\n", before_lines.join("\n"));
        let produced = format!("{}\n", produced_lines.join("\n"));
        std::fs::write(&file, &before)?;
        std::fs::write(&file, produced)?;

        let rejected = apply_vicinity_after_constraints(&file, "rik", &before)?;

        assert_eq!(rejected, 1);
        let expected = std::iter::once("stray original".to_string())
            .chain((2..=25).map(|line| format!("line {line}")))
            .chain(["rik: task".to_string(), "near changed".to_string()])
            .collect::<Vec<_>>();
        assert_eq!(
            std::fs::read_to_string(&file)?,
            format!("{}\n", expected.join("\n"))
        );
        Ok(())
    }

    #[test]
    fn vicinity_after_keeps_replacing_marker_with_content() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        let before = "before\nrik: task\nafter\n";
        std::fs::write(&file, before)?;
        std::fs::write(&file, "before\ninserted\nafter\n")?;

        let rejected = apply_vicinity_after_constraints(&file, "rik", before)?;

        assert_eq!(rejected, 0);
        assert_eq!(std::fs::read_to_string(&file)?, "before\ninserted\nafter\n");
        Ok(())
    }

    #[test]
    fn vicinity_after_restores_insertion_that_only_appends_at_marker() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        let before = "before\nrik: task\nafter\n";
        std::fs::write(&file, before)?;
        std::fs::write(&file, "before\nrik: task\ninserted\nafter\n")?;

        let rejected = apply_vicinity_after_constraints(&file, "rik", before)?;

        assert_eq!(rejected, 1);
        assert_eq!(std::fs::read_to_string(&file)?, before);
        Ok(())
    }

    #[test]
    fn vicinity_after_keeps_previous_line_modification_near_marker() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.txt");
        let before_lines = std::iter::once("stray original".to_string())
            .chain((2..=25).map(|line| format!("line {line}")))
            .chain([
                "previous original".to_string(),
                "rik: task".to_string(),
                "after".to_string(),
            ])
            .collect::<Vec<_>>();
        let produced_lines = std::iter::once("stray changed".to_string())
            .chain((2..=25).map(|line| format!("line {line}")))
            .chain([
                "previous changed".to_string(),
                "rik: task".to_string(),
                "after".to_string(),
            ])
            .collect::<Vec<_>>();
        let before = format!("{}\n", before_lines.join("\n"));
        let produced = format!("{}\n", produced_lines.join("\n"));
        std::fs::write(&file, &before)?;
        std::fs::write(&file, produced)?;

        let rejected = apply_vicinity_after_constraints(&file, "rik", &before)?;

        assert_eq!(rejected, 1);
        let expected = std::iter::once("stray original".to_string())
            .chain((2..=25).map(|line| format!("line {line}")))
            .chain([
                "previous changed".to_string(),
                "rik: task".to_string(),
                "after".to_string(),
            ])
            .collect::<Vec<_>>();
        assert_eq!(
            std::fs::read_to_string(&file)?,
            format!("{}\n", expected.join("\n"))
        );
        Ok(())
    }

    #[test]
    fn context_cleanup_leaves_stopped_context_markers() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("markers.rs");
        std::fs::write(
            &file,
            "!rik: /keep this context/\nrik: /remove this context/\ncontent\n",
        )?;

        remove_context_markers(&file, "rik")?;

        assert_eq!(
            std::fs::read_to_string(&file)?,
            "!rik: /keep this context/\ncontent\n"
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
    no_ignore: bool,
) -> std::collections::HashMap<std::path::PathBuf, u64> {
    let mut hashes = std::collections::HashMap::new();
    if let Ok(files) = crate::helpers::expand_glob(app_state, pattern, no_ignore) {
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
    no_ignore: bool,
    prev: &std::collections::HashMap<std::path::PathBuf, u64>,
) -> bool {
    if let Ok(files) = crate::helpers::expand_glob(app_state, pattern, no_ignore) {
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
    no_ignore: bool,
    system_prompt: Option<&str>,
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
        no_ignore,
        system_prompt,
    )
    .await;
    let mut prev_hashes = snapshot_hashes(app_state, &pattern, no_ignore);

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
                if !files_changed(app_state, &pattern, no_ignore, &prev_hashes) {
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
                    no_ignore,
                    system_prompt,
                )
                .await
                {
                    eprintln!("Watch error: {e:?}");
                }
                prev_hashes = snapshot_hashes(app_state, &pattern, no_ignore);
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
