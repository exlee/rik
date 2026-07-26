use rig::client::CompletionClient;
use rig::completion::Prompt;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Session memory
// ---------------------------------------------------------------------------
//
// Every finished turn is kept whole in memory under an id: the request, the
// reasoning, the output, and — in its own store — the diff. Only a short
// summary of each turn goes into the prompt, so far more turns fit in the
// budget than their full text ever would. The agent pulls back whatever detail
// it actually needs with the `recall` tool.
//
// When the summaries outgrow the budget, the oldest two thirds are merged by
// the model into a single summary and their raw detail is released; the newest
// third follows it untouched.

/// Longest turn transcript handed to the summarizer, in characters.
const MAX_TURN_LOG_CHARS: usize = 24_000;
/// Longest reasoning and output text kept per turn, in characters.
const MAX_RECALL_CHARS: usize = 40_000;
/// Longest diff kept per turn, in characters.
const MAX_DIFF_CHARS: usize = 40_000;
/// Longest answer kept verbatim as a question turn's summary, in characters.
const MAX_ANSWER_CHARS: usize = 4_000;

/// Rough token estimate. Good enough for budgeting; ~4 characters per token.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// What kind of turn a memory came from, which decides how it reads back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnKind {
    /// An editing task: instruction, compacted account of the work, diff.
    Task,
    /// A question marker: the question and the answer given, kept verbatim.
    Question,
}

impl TurnKind {
    /// Labels for the two summarized fields.
    fn labels(self) -> (&'static str, &'static str) {
        match self {
            Self::Task => ("Request", "Summary"),
            Self::Question => ("Question", "Answer"),
        }
    }
}

/// One remembered turn. Only `request` and `summary` reach the prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub id: usize,
    pub kind: TurnKind,
    /// The marker instruction, or the question that was asked.
    pub request: String,
    /// The model's compacted account of the work — or, for a question, its answer.
    pub summary: String,
    /// Everything the model thought during the turn.
    pub reasoning: String,
    /// Everything the model said and every tool it ran.
    pub output: String,
}

impl Turn {
    fn render(&self) -> String {
        let (request_label, summary_label) = self.kind.labels();
        format!(
            "### Memory {}\n{request_label}: {}\n{summary_label}: {}",
            self.id,
            self.request.trim(),
            self.summary.trim()
        )
    }
}

fn turns() -> std::sync::MutexGuard<'static, Vec<Turn>> {
    static TURNS: OnceLock<Mutex<Vec<Turn>>> = OnceLock::new();
    TURNS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Diffs live apart from the turns: they never enter a prompt, only `recall`.
fn diffs() -> std::sync::MutexGuard<'static, HashMap<usize, String>> {
    static DIFFS: OnceLock<Mutex<HashMap<usize, String>>> = OnceLock::new();
    DIFFS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn next_id() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Estimated size of the rendered history, in tokens.
pub fn used_tokens() -> usize {
    estimate_tokens(&history_block())
}

/// How many turns are remembered right now.
pub fn len() -> usize {
    turns().len()
}

/// The `History` section appended to a prompt, empty when nothing is remembered.
pub fn history_block() -> String {
    let rendered = render_all(&turns());
    if rendered.is_empty() {
        return String::new();
    }
    format!(
        "\n\nHistory — work already completed in this session, oldest first. \
         Use it for context and consistency; do not redo it.\n\
         Each memory is summarized. To read one in full — the original request, \
         the reasoning, the output, or the diff — call the `recall` tool with \
         that memory's id.\n\n{rendered}"
    )
}

fn render_all(turns: &[Turn]) -> String {
    turns
        .iter()
        .map(Turn::render)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Forget everything. Used by tests.
#[cfg(test)]
pub fn clear() {
    turns().clear();
    diffs().clear();
}

/// Keep only the first `max_chars` characters, marking what was cut.
fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let kept: String = text.chars().take(max_chars).collect();
    format!("{kept}\n[... truncated ...]")
}

/// Plain unified-style diff of a single file, for the model to read.
///
/// The shared prefix and suffix are trimmed and everything that changed in
/// between is emitted as one hunk with three lines of context. That is exact
/// for the localized edits rik makes and stays readable for scattered ones,
/// without shelling out to a diff tool or pulling in a diff crate.
pub fn plain_diff(before: &str, after: &str) -> String {
    if before == after {
        return String::new();
    }
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();

    let mut head = 0;
    while head < old.len() && head < new.len() && old[head] == new[head] {
        head += 1;
    }
    let mut tail = 0;
    while tail < old.len() - head
        && tail < new.len() - head
        && old[old.len() - 1 - tail] == new[new.len() - 1 - tail]
    {
        tail += 1;
    }

    const CONTEXT: usize = 3;
    let removed = &old[head..old.len() - tail];
    let added = &new[head..new.len() - tail];
    let before_context = &old[head.saturating_sub(CONTEXT)..head];
    let after_start = old.len() - tail;
    let after_context = &old[after_start..(after_start + CONTEXT).min(old.len())];

    let mut diff = format!(
        "@@ -{},{} +{},{} @@\n",
        head + 1,
        removed.len(),
        head + 1,
        added.len()
    );
    for line in before_context {
        diff.push_str(&format!(" {line}\n"));
    }
    for line in removed {
        diff.push_str(&format!("-{line}\n"));
    }
    for line in added {
        diff.push_str(&format!("+{line}\n"));
    }
    for line in after_context {
        diff.push_str(&format!(" {line}\n"));
    }
    diff
}

// ---------------------------------------------------------------------------
// Recall
// ---------------------------------------------------------------------------

/// Which parts of a remembered turn the caller wants back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Recall {
    pub request: bool,
    pub reasoning: bool,
    pub output: bool,
    pub diff: bool,
}

impl Recall {
    /// Asking for nothing in particular means asking for everything.
    fn or_everything(self) -> Self {
        if self == Self::default() {
            Self {
                request: true,
                reasoning: true,
                output: true,
                diff: true,
            }
        } else {
            self
        }
    }
}

/// Read back parts of one remembered turn.
///
/// Errors carry the list of ids that are still available, so a wrong guess
/// costs the agent one turn rather than leaving it stuck.
pub fn recall(id: usize, wanted: Recall) -> Result<String, String> {
    let wanted = wanted.or_everything();
    let turns = turns();
    let Some(turn) = turns.iter().find(|turn| turn.id == id) else {
        let available: Vec<String> = turns.iter().map(|turn| turn.id.to_string()).collect();
        return Err(if available.is_empty() {
            "Nothing is remembered yet.".to_string()
        } else {
            format!("No memory {id}. Available: {}.", available.join(", "))
        });
    };

    let mut sections = vec![format!("Memory {}", turn.id)];
    let (request_label, summary_label) = turn.kind.labels();
    if wanted.request {
        sections.push(format!("{request_label}:\n{}", turn.request.trim()));
    }
    sections.push(format!("{summary_label}:\n{}", turn.summary.trim()));
    if wanted.reasoning {
        sections.push(format!("Reasoning:\n{}", section(&turn.reasoning)));
    }
    if wanted.output {
        sections.push(format!("Output:\n{}", section(&turn.output)));
    }
    if wanted.diff {
        let diff = diffs().get(&id).cloned().unwrap_or_default();
        sections.push(format!("Diff:\n{}", section(&diff)));
    }
    Ok(sections.join("\n\n"))
}

/// Detail released when a turn was merged into an older memory reads as absent,
/// not as an error — the memory itself is still there.
fn section(text: &str) -> &str {
    if text.trim().is_empty() {
        "(not kept for this memory)"
    } else {
        text.trim()
    }
}

// ---------------------------------------------------------------------------
// Turn log
// ---------------------------------------------------------------------------

/// Everything the agent produced during one turn, split the way `recall` serves it.
///
/// Collected regardless of `--verbose`, because the summarizer reads it even
/// when the user does not.
#[derive(Default, Debug)]
pub struct TurnLog {
    reasoning: String,
    output: String,
}

impl TurnLog {
    pub fn reasoning(&mut self, text: &str) {
        self.reasoning.push_str(text);
    }

    pub fn output(&mut self, text: &str) {
        if !self.output.is_empty() && !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        self.output.push_str(text);
    }

    pub fn tool(&mut self, line: &str) {
        self.output(&format!("[tool] {line}"));
    }

    /// The whole turn as one text, for the summarizer.
    pub fn transcript(&self) -> String {
        format!(
            "[thinking]\n{}\n\n[output]\n{}",
            self.reasoning.trim(),
            self.output.trim()
        )
    }
}

// ---------------------------------------------------------------------------
// Summarization
// ---------------------------------------------------------------------------

const TURN_PREAMBLE: &str = "\
You compact one completed editing turn into a single durable memory summary.

Cover exactly three things, briefly and factually:
- WHAT was done (the change, in concrete terms).
- WHY it was done (the intent behind the instruction).
- HOW it was done (approach, files touched, decisions and trade-offs made).

Under 200 words. No preamble, no headings, no praise — output only the summary text.";

const MERGE_PREAMBLE: &str = "\
You merge several memory summaries from one working session into a single summary.

Keep what a later turn would need: what was built, why, and the conventions and \
decisions that later work must stay consistent with. Drop incidental detail and \
anything superseded by a later memory. Preserve chronology where it matters.

Under 250 words. No preamble, no headings — output only the merged summary text.";

/// Ask the model to compact one finished task turn, then remember it.
///
/// A memory limit of 0 disables the whole mechanism. Failures are reported to
/// the caller and never abort the task the memory describes.
#[allow(clippy::too_many_arguments)]
pub async fn remember<C>(
    client: &C,
    model_name: &str,
    limit_tokens: usize,
    request: &str,
    file: &str,
    turn_log: &TurnLog,
    diff: &str,
) -> anyhow::Result<()>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    if limit_tokens == 0 {
        return Ok(());
    }

    let transcript = turn_log.transcript();
    let prompt = format!(
        "Instruction: {request}\nFile: {file}\n\nTurn log:\n{}\n\nDiff:\n{}",
        truncate(&transcript, MAX_TURN_LOG_CHARS),
        truncate(diff, MAX_TURN_LOG_CHARS),
    );
    let summary = client
        .agent(model_name)
        .preamble(TURN_PREAMBLE)
        .build()
        .prompt(prompt)
        .await?;

    let id = next_id();
    if !diff.is_empty() {
        diffs().insert(id, truncate(diff, MAX_DIFF_CHARS));
    }
    turns().push(Turn {
        id,
        kind: TurnKind::Task,
        request: request.to_string(),
        summary,
        reasoning: truncate(&turn_log.reasoning, MAX_RECALL_CHARS),
        output: truncate(&turn_log.output, MAX_RECALL_CHARS),
    });

    enforce_limit(client, model_name, limit_tokens).await
}

/// Remember an answered question turn: the question and the answer, verbatim.
///
/// No summarizing call — a question marker already produces a short, concise
/// answer, and paraphrasing it would only lose detail. Long answers are capped.
pub async fn remember_answer<C>(
    client: &C,
    model_name: &str,
    limit_tokens: usize,
    question: &str,
    answer: &str,
    turn_log: &TurnLog,
) -> anyhow::Result<()>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    if limit_tokens == 0 {
        return Ok(());
    }

    turns().push(Turn {
        id: next_id(),
        kind: TurnKind::Question,
        request: question.to_string(),
        summary: truncate(answer, MAX_ANSWER_CHARS),
        reasoning: truncate(&turn_log.reasoning, MAX_RECALL_CHARS),
        output: truncate(&turn_log.output, MAX_RECALL_CHARS),
    });

    enforce_limit(client, model_name, limit_tokens).await
}

/// How many of the oldest memories are merged when the budget is exceeded.
///
/// Two thirds, rounded down, but never fewer than two — merging a single memory
/// could leave the count unchanged and loop forever.
fn merge_count(len: usize) -> usize {
    ((len * 2) / 3).max(2).min(len)
}

/// Bring the history back under `limit_tokens`.
///
/// Merges the oldest two thirds into one summary, repeatedly if needed, and
/// releases the raw detail of everything it merged. If a single memory still
/// does not fit, memories are dropped without asking the model — the budget is
/// a hard ceiling.
async fn enforce_limit<C>(client: &C, model_name: &str, limit_tokens: usize) -> anyhow::Result<()>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    while used_tokens() > limit_tokens && len() >= 2 {
        let (old, recent) = take_oldest_for_merge();

        match merge_notes(client, model_name, &old).await {
            Ok(merged) => {
                forget_detail(&old);
                restore(vec![merged], recent);
            }
            Err(error) => {
                // Put the history back untouched, then fall through to the
                // model-free shrinking below so the budget still holds.
                restore(old, recent);
                shrink_without_model(limit_tokens);
                return Err(error);
            }
        }
    }

    shrink_without_model(limit_tokens);
    Ok(())
}

/// Detach the oldest two thirds for merging, leaving the store empty.
///
/// Returns `(to_merge, to_keep)`; the caller must put both back with [`restore`].
fn take_oldest_for_merge() -> (Vec<Turn>, Vec<Turn>) {
    let mut turns = turns();
    let split = merge_count(turns.len());
    let recent = turns.split_off(split);
    (std::mem::take(&mut *turns), recent)
}

/// Put turns back into the store, oldest group first.
fn restore(older: Vec<Turn>, newer: Vec<Turn>) {
    let mut turns = turns();
    turns.extend(older);
    turns.extend(newer);
}

/// Release the recallable detail of turns that were folded into a summary.
fn forget_detail(merged: &[Turn]) {
    let mut diffs = diffs();
    for turn in merged {
        diffs.remove(&turn.id);
    }
}

/// Merge summaries into one, dropping the raw detail behind them.
async fn merge_notes<C>(client: &C, model_name: &str, old: &[Turn]) -> anyhow::Result<Turn>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    let rendered: Vec<String> = old.iter().map(Turn::render).collect();
    let summary = client
        .agent(model_name)
        .preamble(MERGE_PREAMBLE)
        .build()
        .prompt(rendered.join("\n\n"))
        .await?;

    Ok(Turn {
        id: next_id(),
        kind: TurnKind::Task,
        request: format!("(merged from {} earlier turns)", old.len()),
        summary,
        reasoning: String::new(),
        output: String::new(),
    })
}

/// Last-resort trimming that needs no model call: drop whole memories
/// oldest-first, then truncate what remains.
fn shrink_without_model(limit_tokens: usize) {
    let mut turns = turns();
    let mut dropped = Vec::new();
    while turns.len() > 1 && estimate_tokens(&render_all(&turns)) > limit_tokens {
        dropped.push(turns.remove(0));
    }
    if estimate_tokens(&render_all(&turns)) > limit_tokens
        && let Some(turn) = turns.first_mut()
    {
        turn.summary = truncate(&turn.summary, limit_tokens * 4);
    }
    drop(turns);
    forget_detail(&dropped);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Memory is global state, so tests that touch it must not run in parallel.
    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn task(request: &str, summary: &str) -> Turn {
        Turn {
            id: next_id(),
            kind: TurnKind::Task,
            request: request.to_string(),
            summary: summary.to_string(),
            reasoning: "because".to_string(),
            output: "[tool] edit_file src/main.rs".to_string(),
        }
    }

    fn remember_task(request: &str, summary: &str, diff: &str) -> usize {
        let turn = task(request, summary);
        let id = turn.id;
        diffs().insert(id, diff.to_string());
        turns().push(turn);
        id
    }

    #[test]
    fn history_is_empty_until_something_is_remembered() {
        let _serialized = test_lock();
        clear();

        assert_eq!(history_block(), "");
        assert_eq!(used_tokens(), 0);
    }

    #[test]
    fn history_carries_summaries_and_points_at_the_recall_tool() {
        let _serialized = test_lock();
        clear();
        let id = remember_task("add auth", "added middleware", "@@\n+auth\n");

        let block = history_block();

        assert!(block.contains("`recall` tool"), "{block}");
        assert!(block.contains(&format!("### Memory {id}")));
        assert!(block.contains("Request: add auth"));
        assert!(block.contains("Summary: added middleware"));
        assert!(
            !block.contains("+auth"),
            "diffs stay out of the prompt entirely"
        );
        clear();
    }

    #[test]
    fn questions_are_remembered_with_their_answer() {
        let _serialized = test_lock();
        clear();
        turns().push(Turn {
            id: next_id(),
            kind: TurnKind::Question,
            request: "is the sky blue?".to_string(),
            summary: "Yes, by Rayleigh scattering.".to_string(),
            reasoning: "thought about it".to_string(),
            output: "Yes, by Rayleigh scattering.".to_string(),
        });

        let block = history_block();

        assert!(block.contains("Question: is the sky blue?"), "{block}");
        assert!(block.contains("Answer: Yes, by Rayleigh scattering."));
        clear();
    }

    #[test]
    fn recall_returns_only_the_requested_parts() {
        let _serialized = test_lock();
        clear();
        let id = remember_task("add auth", "added middleware", "@@\n+auth\n");

        let diff_only = recall(
            id,
            Recall {
                diff: true,
                ..Recall::default()
            },
        )
        .unwrap();

        assert!(diff_only.contains("+auth"));
        assert!(diff_only.contains("Summary:"), "summary always comes along");
        assert!(!diff_only.contains("Reasoning:"));
        assert!(!diff_only.contains("Request:"));
        clear();
    }

    #[test]
    fn recall_without_flags_returns_everything() {
        let _serialized = test_lock();
        clear();
        let id = remember_task("add auth", "added middleware", "@@\n+auth\n");

        let all = recall(id, Recall::default()).unwrap();

        assert!(all.contains("Request:\nadd auth"));
        assert!(all.contains("Reasoning:\nbecause"));
        assert!(all.contains("Output:\n[tool] edit_file"));
        assert!(all.contains("+auth"));
        clear();
    }

    #[test]
    fn recall_of_an_unknown_id_lists_what_is_available() {
        let _serialized = test_lock();
        clear();
        let id = remember_task("add auth", "added middleware", "");

        let error = recall(id + 999, Recall::default()).unwrap_err();

        assert!(error.contains(&format!("Available: {id}")), "{error}");
        clear();
    }

    #[test]
    fn recall_reports_detail_that_was_released_by_a_merge() {
        let _serialized = test_lock();
        clear();
        let id = remember_task("add auth", "added middleware", "@@\n+auth\n");
        let merged = turns().clone();
        forget_detail(&merged);

        let all = recall(id, Recall::default()).unwrap();

        assert!(all.contains("Diff:\n(not kept for this memory)"), "{all}");
        clear();
    }

    #[test]
    fn merge_takes_two_thirds_and_never_fewer_than_two() {
        assert_eq!(merge_count(2), 2);
        assert_eq!(merge_count(3), 2);
        assert_eq!(merge_count(6), 4);
        assert_eq!(merge_count(9), 6);
        assert_eq!(merge_count(10), 6);
    }

    #[test]
    fn overflow_merges_the_oldest_two_thirds_and_keeps_the_newest_third() {
        let _serialized = test_lock();
        clear();
        let ids: Vec<usize> = (1..=6)
            .map(|index| {
                remember_task(
                    &format!("task {index}"),
                    &format!("did {index}"),
                    "@@\n+x\n",
                )
            })
            .collect();

        // What enforce_limit does around the model call.
        let (old, recent) = take_oldest_for_merge();
        assert_eq!(old.len(), 4);
        assert_eq!(recent.len(), 2);
        assert_eq!(old[0].request, "task 1");
        assert_eq!(recent[0].request, "task 5");

        forget_detail(&old);
        restore(
            vec![Turn {
                id: next_id(),
                kind: TurnKind::Task,
                request: format!("(merged from {} earlier turns)", old.len()),
                summary: "everything before".to_string(),
                reasoning: String::new(),
                output: String::new(),
            }],
            recent,
        );

        let turns = turns();
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0].request, "(merged from 4 earlier turns)");
        assert_eq!(turns[1].request, "task 5");
        assert_eq!(turns[2].request, "task 6");
        drop(turns);
        // Merged-away detail is released; what survived keeps its diff.
        assert!(!diffs().contains_key(&ids[0]));
        assert!(diffs().contains_key(&ids[5]));
        clear();
    }

    #[test]
    fn shrinking_drops_the_oldest_memories_and_their_detail() {
        let _serialized = test_lock();
        clear();
        let first = remember_task("first", &"x".repeat(400), "@@\n+x\n");
        remember_task("second", "short", "");

        shrink_without_model(40);

        let turns = turns();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].request, "second");
        drop(turns);
        assert!(!diffs().contains_key(&first));
        clear();
    }

    #[test]
    fn shrinking_truncates_a_single_oversized_memory() {
        let _serialized = test_lock();
        clear();
        remember_task("only", &"x".repeat(4_000), "");

        shrink_without_model(100);

        assert!(used_tokens() <= 200, "still {} tokens", used_tokens());
        clear();
    }

    #[test]
    fn plain_diff_reports_only_the_changed_middle() {
        let before = "one\ntwo\nthree\nfour\n";
        let after = "one\ntwo\nCHANGED\nfour\n";

        let diff = plain_diff(before, after);

        assert!(diff.starts_with("@@ -3,1 +3,1 @@\n"), "{diff}");
        assert!(diff.contains("-three\n"));
        assert!(diff.contains("+CHANGED\n"));
        assert!(diff.contains(" one\n") && diff.contains(" four\n"));
    }

    #[test]
    fn plain_diff_of_identical_content_is_empty() {
        assert_eq!(plain_diff("same\n", "same\n"), "");
    }

    #[test]
    fn plain_diff_handles_pure_insertion_and_deletion() {
        assert!(plain_diff("a\nb\n", "a\nnew\nb\n").contains("+new\n"));
        assert!(plain_diff("a\ngone\nb\n", "a\nb\n").contains("-gone\n"));
    }

    #[test]
    fn turn_log_keeps_reasoning_and_output_apart() {
        let mut log = TurnLog::default();
        log.reasoning("think");
        log.reasoning("ing");
        log.output("done");
        log.tool("read_file src/main.rs");

        assert_eq!(log.reasoning, "thinking");
        assert_eq!(log.output, "done\n[tool] read_file src/main.rs");
        assert!(log.transcript().contains("[thinking]\nthinking"));
        assert!(log.transcript().contains("[output]\ndone"));
    }

    #[test]
    fn token_estimate_is_four_characters_per_token() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }
}
