use rig::client::CompletionClient;
use rig::completion::Prompt;
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Session memory
// ---------------------------------------------------------------------------
//
// Every completed turn is compacted by the model into one note (what was done,
// why, how). Notes are replayed to later turns as a "History" section appended
// to the prompt. The whole block is kept under a configurable token budget: on
// overflow the oldest two thirds are merged into a single note — without their
// diffs — and the newest third follows it unchanged.

/// Longest turn log handed to the summarizer, in characters.
const MAX_TURN_LOG_CHARS: usize = 24_000;
/// Longest diff kept in a note (and shown to the summarizer), in characters.
const MAX_DIFF_CHARS: usize = 6_000;

/// Rough token estimate. Good enough for budgeting; ~4 characters per token.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// One remembered turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// The user's marker instruction that started the turn.
    pub instruction: String,
    /// The model's compacted account of what was done, why, and how.
    pub summary: String,
    /// The turn's diff. Dropped once the note is merged into an older one.
    pub diff: Option<String>,
}

impl Note {
    fn render(&self, index: usize) -> String {
        let mut rendered = format!(
            "### Memory {index}\nInstruction: {}\nSummary: {}",
            self.instruction.trim(),
            self.summary.trim()
        );
        if let Some(diff) = &self.diff {
            rendered.push_str("\nDiff:\n");
            rendered.push_str(diff.trim_end());
        }
        rendered
    }
}

fn store() -> &'static Mutex<Vec<Note>> {
    static STORE: OnceLock<Mutex<Vec<Note>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Vec::new()))
}

fn notes() -> std::sync::MutexGuard<'static, Vec<Note>> {
    store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Estimated size of the rendered history, in tokens.
pub fn used_tokens() -> usize {
    estimate_tokens(&history_block())
}

/// The `History` section appended to a task prompt, empty when nothing is remembered.
pub fn history_block() -> String {
    let notes = notes();
    if notes.is_empty() {
        return String::new();
    }
    let rendered: Vec<String> = notes
        .iter()
        .enumerate()
        .map(|(index, note)| note.render(index + 1))
        .collect();
    format!(
        "\n\nHistory — work already completed in this session, oldest first. \
         Use it for context and consistency; do not redo it.\n\n{}",
        rendered.join("\n\n")
    )
}

/// Forget everything. Used by tests.
#[cfg(test)]
pub fn clear() {
    notes().clear();
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
    truncate(&diff, MAX_DIFF_CHARS)
}

// ---------------------------------------------------------------------------
// Turn log
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Reasoning,
    Output,
    Tool,
}

impl Kind {
    fn header(self) -> &'static str {
        match self {
            Self::Reasoning => "\n[thinking]\n",
            Self::Output => "\n[assistant]\n",
            Self::Tool => "\n[tools]\n",
        }
    }
}

/// Everything the agent produced during one turn: reasoning, replies, tool traffic.
///
/// Collected regardless of `--verbose`, because the summarizer reads it even
/// when the user does not.
#[derive(Default, Debug)]
pub struct TurnLog {
    buffer: String,
    last: Option<Kind>,
}

impl TurnLog {
    fn push(&mut self, kind: Kind, text: &str) {
        if self.last != Some(kind) {
            self.buffer.push_str(kind.header());
            self.last = Some(kind);
        }
        self.buffer.push_str(text);
    }

    pub fn reasoning(&mut self, text: &str) {
        self.push(Kind::Reasoning, text);
    }

    pub fn output(&mut self, text: &str) {
        self.push(Kind::Output, text);
    }

    pub fn tool(&mut self, line: &str) {
        self.push(Kind::Tool, &format!("{line}\n"));
    }

    pub fn as_str(&self) -> &str {
        &self.buffer
    }
}

// ---------------------------------------------------------------------------
// Summarization
// ---------------------------------------------------------------------------

const TURN_PREAMBLE: &str = "\
You compact one completed editing turn into a single durable memory note.

Cover exactly three things, briefly and factually:
- WHAT was done (the change, in concrete terms).
- WHY it was done (the intent behind the instruction).
- HOW it was done (approach, files touched, decisions and trade-offs made).

Under 200 words. No preamble, no headings, no praise — output only the note text.";

const MERGE_PREAMBLE: &str = "\
You merge several memory notes from one working session into a single note.

Keep what a later turn would need: what was built, why, and the conventions and \
decisions that later work must stay consistent with. Drop incidental detail and \
anything superseded by a later note. Preserve chronology where it matters.

Under 250 words. No preamble, no headings — output only the merged note text.";

/// Ask the model to compact one finished turn, then store it as a note.
///
/// A memory limit of 0 disables the whole mechanism. Failures are reported to
/// the caller and never abort the task the note describes.
pub async fn remember<C>(
    client: &C,
    model_name: &str,
    limit_tokens: usize,
    instruction: &str,
    file: &str,
    turn_log: &str,
    diff: &str,
) -> anyhow::Result<()>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    if limit_tokens == 0 {
        return Ok(());
    }

    let prompt = format!(
        "Instruction: {instruction}\nFile: {file}\n\nTurn log:\n{}\n\nDiff:\n{}",
        truncate(turn_log, MAX_TURN_LOG_CHARS),
        truncate(diff, MAX_DIFF_CHARS),
    );
    let summary = client
        .agent(model_name)
        .preamble(TURN_PREAMBLE)
        .build()
        .prompt(prompt)
        .await?;

    notes().push(Note {
        instruction: instruction.to_string(),
        summary,
        diff: (!diff.is_empty()).then(|| truncate(diff, MAX_DIFF_CHARS)),
    });

    enforce_limit(client, model_name, limit_tokens).await
}

/// How many of the oldest notes are merged when the budget is exceeded.
///
/// Two thirds, rounded down, but never fewer than two — merging a single note
/// could leave the count unchanged and loop forever.
fn merge_count(len: usize) -> usize {
    ((len * 2) / 3).max(2).min(len)
}

/// Bring the history back under `limit_tokens`.
///
/// Merges the oldest two thirds into one diff-less note, repeatedly if needed.
/// If a single note still does not fit, diffs and then whole notes are dropped
/// without asking the model — the budget is a hard ceiling.
async fn enforce_limit<C>(client: &C, model_name: &str, limit_tokens: usize) -> anyhow::Result<()>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    while used_tokens() > limit_tokens && notes().len() >= 2 {
        let (old, recent) = take_oldest_for_merge();

        match merge_notes(client, model_name, &old).await {
            Ok(merged) => restore(vec![merged], recent),
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
fn take_oldest_for_merge() -> (Vec<Note>, Vec<Note>) {
    let mut notes = notes();
    let split = merge_count(notes.len());
    let recent = notes.split_off(split);
    (std::mem::take(&mut *notes), recent)
}

/// Put notes back into the store, oldest group first.
fn restore(older: Vec<Note>, newer: Vec<Note>) {
    let mut notes = notes();
    notes.extend(older);
    notes.extend(newer);
}

/// Merge notes into one, keeping the instructions but dropping the diffs.
async fn merge_notes<C>(client: &C, model_name: &str, old: &[Note]) -> anyhow::Result<Note>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    let rendered: Vec<String> = old
        .iter()
        .enumerate()
        .map(|(index, note)| {
            format!(
                "### Note {}\nInstruction: {}\nSummary: {}",
                index + 1,
                note.instruction.trim(),
                note.summary.trim()
            )
        })
        .collect();

    let summary = client
        .agent(model_name)
        .preamble(MERGE_PREAMBLE)
        .build()
        .prompt(rendered.join("\n\n"))
        .await?;

    Ok(Note {
        instruction: format!("(merged from {} earlier tasks)", old.len()),
        summary,
        diff: None,
    })
}

/// Last-resort trimming that needs no model call: drop diffs oldest-first,
/// then whole notes oldest-first, then truncate what remains.
fn shrink_without_model(limit_tokens: usize) {
    let mut notes = notes();
    let mut index = 0;
    while index < notes.len() && estimate_tokens(&render_all(&notes)) > limit_tokens {
        notes[index].diff = None;
        index += 1;
    }
    while notes.len() > 1 && estimate_tokens(&render_all(&notes)) > limit_tokens {
        notes.remove(0);
    }
    if estimate_tokens(&render_all(&notes)) > limit_tokens
        && let Some(note) = notes.first_mut()
    {
        note.summary = truncate(&note.summary, limit_tokens * 4);
    }
}

fn render_all(notes: &[Note]) -> String {
    notes
        .iter()
        .enumerate()
        .map(|(index, note)| note.render(index + 1))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Memory is global state, so tests that touch it must not run in parallel.
    pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn note(instruction: &str, summary: &str, diff: Option<&str>) -> Note {
        Note {
            instruction: instruction.to_string(),
            summary: summary.to_string(),
            diff: diff.map(str::to_string),
        }
    }

    #[test]
    fn history_is_empty_until_something_is_remembered() {
        let _serialized = test_lock();
        clear();

        assert_eq!(history_block(), "");
        assert_eq!(used_tokens(), 0);
    }

    #[test]
    fn history_renders_instruction_summary_and_diff_oldest_first() {
        let _serialized = test_lock();
        clear();
        notes().push(note("add auth", "added middleware", Some("@@\n+auth\n")));
        notes().push(note("add tests", "covered the middleware", None));

        let block = history_block();

        assert!(block.starts_with("\n\nHistory"));
        assert!(block.contains("### Memory 1\nInstruction: add auth"));
        assert!(block.contains("Summary: added middleware"));
        assert!(block.contains("Diff:\n@@\n+auth"));
        assert!(block.contains("### Memory 2\nInstruction: add tests"));
        assert!(block.find("add auth") < block.find("add tests"));
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
        for index in 1..=6 {
            notes().push(note(
                &format!("task {index}"),
                &format!("did {index}"),
                Some("@@\n+x\n"),
            ));
        }

        // What enforce_limit does around the model call.
        let (old, recent) = take_oldest_for_merge();
        assert_eq!(old.len(), 4);
        assert_eq!(recent.len(), 2);
        assert_eq!(old[0].instruction, "task 1");
        assert_eq!(recent[0].instruction, "task 5");

        restore(
            vec![Note {
                instruction: format!("(merged from {} earlier tasks)", old.len()),
                summary: "everything before".to_string(),
                diff: None,
            }],
            recent,
        );

        let notes = notes();
        assert_eq!(notes.len(), 3);
        assert_eq!(notes[0].instruction, "(merged from 4 earlier tasks)");
        assert!(notes[0].diff.is_none(), "merged notes drop their diffs");
        assert_eq!(notes[1].instruction, "task 5");
        assert_eq!(notes[2].instruction, "task 6");
        drop(notes);
        clear();
    }

    #[test]
    fn shrinking_drops_diffs_before_notes() {
        let _serialized = test_lock();
        clear();
        let filler = "x".repeat(400);
        notes().push(note("first", "kept", Some(&filler)));
        notes().push(note("second", "kept", Some(&filler)));

        // Enough room for both summaries, not for the diffs.
        shrink_without_model(60);

        let notes = notes();
        assert_eq!(notes.len(), 2);
        assert!(notes.iter().all(|note| note.diff.is_none()));
        drop(notes);
        clear();
    }

    #[test]
    fn shrinking_drops_the_oldest_notes_when_diffs_are_not_enough() {
        let _serialized = test_lock();
        clear();
        let filler = "x".repeat(400);
        notes().push(note("first", &filler, None));
        notes().push(note("second", "short", None));

        shrink_without_model(40);

        let notes = notes();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].instruction, "second");
        drop(notes);
        clear();
    }

    #[test]
    fn shrinking_truncates_a_single_oversized_note() {
        let _serialized = test_lock();
        clear();
        notes().push(note("only", &"x".repeat(4_000), None));

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
    fn turn_log_groups_consecutive_chunks_under_one_header() {
        let mut log = TurnLog::default();
        log.reasoning("think");
        log.reasoning("ing");
        log.output("done");
        log.tool("read_file src/main.rs");
        log.tool("edit_file src/main.rs");

        let text = log.as_str();

        assert!(text.contains("[thinking]\nthinking"));
        assert!(text.contains("[assistant]\ndone"));
        assert!(text.contains("[tools]\nread_file src/main.rs\nedit_file src/main.rs\n"));
        assert_eq!(text.matches("[thinking]").count(), 1);
        assert_eq!(text.matches("[tools]").count(), 1);
    }

    #[test]
    fn token_estimate_is_four_characters_per_token() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }
}
