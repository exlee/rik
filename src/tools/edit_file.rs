use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use crate::markers::MarkerKind;
use crate::tools::ReadFileHistory;

// ---------------------------------------------------------------------------
// EditFile tool
// ---------------------------------------------------------------------------

/// Radius (in lines) around a marker span within which edits are allowed.
const MARKER_RADIUS: usize = 7;

/// Arguments for the edit_file tool.
#[derive(Deserialize)]
pub struct EditFileArgs {
    /// Exact text to find in the file. Must be unique.
    pub old_text: String,
    /// Replacement text.
    pub new_text: String,
}

/// Error type for the edit_file tool.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct EditFileError(String);

impl From<std::io::Error> for EditFileError {
    fn from(e: std::io::Error) -> Self {
        EditFileError(e.to_string())
    }
}

/// A tool that performs exact text replacement in the target file.
///
/// Finds `old_text` and replaces it with `new_text`. The `old_text` must match
/// exactly one occurrence.
///
/// Two restrictions are enforced at the code level:
/// - **File scope**: the tool always edits `target_path` — no argument needed.
/// - **Line scope**: the edit must fall within `MARKER_RADIUS` lines of a
///   marker span. Marker positions are re-read from disk on every call so they
///   stay correct even after earlier edits shift line numbers.
#[derive(Deserialize, Serialize)]
pub struct EditFileTool<'a> {
    #[serde(skip, default = "crate::state::get")]
    pub app_state: &'a crate::state::AppState,
    /// The file this tool is allowed to edit (set at construction time).
    pub target_path: String,
    /// Marker alias used to scan the file for current marker positions.
    pub alias: String,
    #[serde(skip, default)]
    pub read_history: Arc<ReadFileHistory>,
}

impl Tool for EditFileTool<'_> {
    const NAME: &'static str = "edit_file";

    type Error = EditFileError;
    type Args = EditFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        let mut desc = format!(
            "Edit {} by replacing exact text. \
             old_text must match exactly one occurrence in the file. \
             That text is replaced with new_text. \
             Only this file may be edited and edits must be within {} lines of a marker.",
            self.target_path, MARKER_RADIUS,
        );
        if self.app_state.config.marker_limits_edition_range {
            desc.push_str(" When multiple markers are present, the edit span has to end before the next marker.");
        }
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: desc,
            parameters: json!({
                "type": "object",
                "properties": {
                    "old_text": {
                        "type": "string",
                        "description": "Exact text to find in the file (must be unique)"
                    },
                    "new_text": {
                        "type": "string",
                        "description": "Replacement text"
                    }
                },
                "required": ["old_text", "new_text"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = self
            .app_state
            .resolve_path(&self.target_path)
            .map_err(|e| EditFileError(e.to_string()))?;
        if !path.exists() {
            return Err(EditFileError(format!("File not found: {}", path.display())));
        }

        let content = std::fs::read_to_string(&path)?;

        // Re-scan markers from current file content so positions are always fresh.
        // Only task markers count as edit anchors; context markers do not.
        let marker_spans: Vec<(usize, usize)> = crate::markers::find_markers(&content, &self.alias)
            .iter()
            .filter(|marker| {
                marker.kind == MarkerKind::Task
                    && !crate::markers::is_stopped(&content, &self.alias, marker)
            })
            .map(|m| (m.start_line, m.end_line))
            .collect();

        let first = content.find(&args.old_text);
        let last = content.rfind(&args.old_text);

        match (first, last) {
            (None, _) => Err(EditFileError(format!(
                "old_text not found in file: {}",
                path.display()
            ))),
            (Some(a), Some(b)) if a != b => Err(EditFileError(format!(
                "old_text matches {} locations in file -- must be unique",
                content.matches(&args.old_text).count()
            ))),
            (Some(pos), _) => {
                // --- Line scope restriction ---
                if !is_edit_near_marker(&marker_spans, &content, pos, &args.old_text) {
                    return Err(EditFileError(format!(
                        "Edit rejected: neither the start nor end line of old_text \
                         is within {} lines of any marker. \
                         Edits must anchor near a marker line.",
                        MARKER_RADIUS,
                    )));
                }

                if removes_multiline_opener_without_closer(
                    &marker_spans,
                    &content,
                    pos,
                    &args.old_text,
                ) {
                    return Err(EditFileError(
                        "Edit rejected: replacing a multiline marker's opening line must also \
                         replace through its closing delimiter."
                            .to_string(),
                    ));
                }

                if self.app_state.config.marker_limits_edition_range
                    && is_edit_over_marker(&marker_spans, &content, pos, &args.old_text)
                {
                    return Err(EditFileError(
                        "Edit rejected: edits must end before the following marker.".to_string(),
                    ));
                }

                let mut new_content = String::with_capacity(
                    content.len() - args.old_text.len() + args.new_text.len(),
                );
                new_content.push_str(&content[..pos]);
                new_content.push_str(&args.new_text);
                new_content.push_str(&content[pos + args.old_text.len()..]);

                std::fs::write(&path, &new_content)?;
                self.read_history.clear();

                Ok(format!(
                    "[edit_file] path={}\nEdited {}",
                    path.display(),
                    path.display()
                ))
            }
        }
    }
}

fn removes_multiline_opener_without_closer(
    marker_spans: &[(usize, usize)],
    content: &str,
    pos: usize,
    old_text: &str,
) -> bool {
    let edit_start_line = byte_offset_to_line(content, pos);
    let edit_end_line = byte_offset_to_line(content, pos + old_text.len().saturating_sub(1));

    marker_spans.iter().any(|&(start, end)| {
        start < end && edit_start_line <= start && edit_end_line >= start && edit_end_line < end
    })
}

// If the marker is on the first line, skip it — there's no preceding content
// so an edit cannot meaningfully "overlap" a marker that starts the file.
fn is_edit_over_marker(
    marker_spans: &[(usize, usize)],
    content: &str,
    pos: usize,
    old_text: &str,
) -> bool {
    let edit_start_line = byte_offset_to_line(content, pos);
    let edit_end_line = byte_offset_to_line(content, pos + old_text.len().saturating_sub(1));

    for &(start, end) in marker_spans {
        if start == edit_start_line {
            continue;
        }
        if edit_start_line >= start && edit_start_line <= end {
            return true;
        }
        if edit_end_line >= start && edit_end_line <= end {
            return true;
        }
    }
    false
}

/// Check whether the edit's start or end line falls within `MARKER_RADIUS`
/// lines of any line inside at least one marker span.
///
/// This mirrors the Prolog rule:
///   possible(edit(Q,P), marker(X,Y)) :-
///       between(X,Y,Z), (in_range(Z,Q) ; in_range(Z,P)).
/// where `in_range(Z, V)` means V ∈ [Z − max_offset, Z + max_offset].
fn is_edit_near_marker(
    marker_spans: &[(usize, usize)],
    content: &str,
    pos: usize,
    old_text: &str,
) -> bool {
    let edit_start_line = byte_offset_to_line(content, pos);
    // Line of the last character of the matched text.
    let edit_end_line = byte_offset_to_line(content, pos + old_text.len().saturating_sub(1));

    for &(start, end) in marker_spans {
        // Iterate every line Z within the marker span [start, end]
        for z in start..=end {
            let lo = z.saturating_sub(MARKER_RADIUS);
            let hi = z + MARKER_RADIUS;
            // Check if either endpoint of the edit is within [lo, hi]
            if (edit_start_line >= lo && edit_start_line <= hi)
                || (edit_end_line >= lo && edit_end_line <= hi)
            {
                return true;
            }
        }
    }
    false
}

/// Convert a byte offset in `content` to a 1-based line number.
fn byte_offset_to_line(content: &str, offset: usize) -> usize {
    let offset = offset.min(content.len());
    // Round down to the nearest char boundary to avoid panicking inside multi-byte UTF-8.
    let offset = content.floor_char_boundary(offset);
    content[..offset].chars().filter(|&c| c == '\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Insert a standalone `rik: do something` marker line before index `before_idx` (0-based).
    /// All subsequent lines shift down by one.
    fn insert_marker_before(lines: &mut Vec<String>, before_idx: usize) {
        let idx = before_idx.min(lines.len());
        lines.insert(idx, "rik: do something".to_string());
    }

    fn make_tool(file_path: &std::path::Path) -> EditFileTool<'static> {
        let app_state = Box::leak(Box::new(
            crate::state::AppState::new(
                file_path.parent().unwrap().to_path_buf(),
                crate::config::Config::default(),
            )
            .unwrap(),
        ));
        EditFileTool {
            app_state,
            target_path: file_path.display().to_string(),
            alias: "rik".to_string(),
            read_history: Arc::default(),
        }
    }

    // Note: this tests checks for default only (i.e. so that limit is enabled)
    //       might require some testing tricks instead
    #[tokio::test]
    async fn test_edit_over_two_markers_rejected() -> anyhow::Result<()> {
        // With marker_limits_edition_range enabled (the default), an edit whose
        // text spans two markers must be rejected.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        // Two markers with unique content between them so old_text matches exactly once.
        std::fs::write(
            &file_path,
            "before\nrik: first task\nmiddle content\nrik: second task\nafter\n",
        )?;

        let tool = make_tool(&file_path);
        // Try to replace text that spans across both markers.
        let result = tool
            .call(EditFileArgs {
                old_text: "rik: first task\nmiddle content\nrik: second task".into(),
                new_text: "replaced everything".into(),
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("edits must end before the following marker"),
            "Expected rejection when edit spans two markers, got: {err}"
        );

        // File must be unchanged
        let content = std::fs::read_to_string(&file_path)?;
        assert!(content.contains("rik: first task"));
        assert!(content.contains("middle content"));
        assert!(content.contains("rik: second task"));
        Ok(())
    }

    #[tokio::test]
    async fn test_injected_config_can_allow_edit_over_two_markers() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(
            &file_path,
            "before\nrik: first task\nmiddle content\nrik: second task\nafter\n",
        )?;
        let mut config = crate::config::Config::default();
        config.marker_limits_edition_range = false;
        let app_state = crate::state::AppState::new(dir.path().to_path_buf(), config)?;
        let tool = EditFileTool {
            app_state: &app_state,
            target_path: file_path.display().to_string(),
            alias: "rik".to_string(),
            read_history: Arc::default(),
        };

        tool.call(EditFileArgs {
            old_text: "rik: first task\nmiddle content\nrik: second task".into(),
            new_text: "replaced everything".into(),
        })
        .await?;

        assert!(std::fs::read_to_string(file_path)?.contains("replaced everything"));
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_file_replace_line() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "before\nrik: write a poem\nafter\n")?;

        let tool = make_tool(&file_path);
        tool.call(EditFileArgs {
            old_text: "rik: write a poem".into(),
            new_text: "Roses are red\nViolets are blue".into(),
        })
        .await?;

        let content = std::fs::read_to_string(&file_path)?;
        assert_eq!(content, "before\nRoses are red\nViolets are blue\nafter\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_file_resets_shared_read_history() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let target_path = dir.path().join("target.txt");
        let context_path = dir.path().join("context.txt");
        std::fs::write(&target_path, "before\nrik: replace me\nafter\n")?;
        std::fs::write(&context_path, "unchanged context")?;

        let app_state = crate::state::AppState::new(
            dir.path().to_path_buf(),
            crate::config::Config::default(),
        )?;
        let read_history = Arc::new(ReadFileHistory::default());
        let read_tool = crate::tools::ReadFileTool::with_history(&app_state, read_history.clone());
        let edit_tool = EditFileTool {
            app_state: &app_state,
            target_path: target_path.display().to_string(),
            alias: "rik".to_string(),
            read_history,
        };
        let read_args = || crate::tools::read_file::ReadFileArgs {
            path: context_path.display().to_string(),
            offset: None,
            limit: None,
        };

        assert!(
            read_tool
                .call(read_args())
                .await?
                .ends_with("unchanged context")
        );
        assert_eq!(
            read_tool.call(read_args()).await.unwrap_err().to_string(),
            "Context known"
        );

        edit_tool
            .call(EditFileArgs {
                old_text: "rik: replace me".into(),
                new_text: "replaced".into(),
            })
            .await?;

        assert!(
            read_tool
                .call(read_args())
                .await?
                .ends_with("unchanged context")
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_file_not_found() {
        let app_state = crate::state::AppState::new(
            std::env::current_dir().unwrap(),
            crate::config::Config::default(),
        )
        .unwrap();
        let tool = EditFileTool {
            app_state: &app_state,
            target_path: "/nonexistent".to_string(),
            alias: "rik".to_string(),
            read_history: Arc::default(),
        };
        let result = tool
            .call(EditFileArgs {
                old_text: "x".into(),
                new_text: "y".into(),
            })
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("outside watched directory")
        );
    }

    #[tokio::test]
    async fn test_edit_file_old_text_missing() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world\nrik: fix this\n")?;

        let tool = make_tool(&file_path);
        let result = tool
            .call(EditFileArgs {
                old_text: "not here".into(),
                new_text: "x".into(),
            })
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_file_duplicate_old_text() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "abc xyz abc\nrik: fix\n")?;

        let tool = make_tool(&file_path);
        let result = tool
            .call(EditFileArgs {
                old_text: "abc".into(),
                new_text: "def".into(),
            })
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unique"));
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_file_prints_tool_name_and_params() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "foo bar baz\nrik: edit\n")?;

        let old_text = "bar";
        let new_text = "qux";

        let tool = make_tool(&file_path);
        let result = tool
            .call(EditFileArgs {
                old_text: old_text.into(),
                new_text: new_text.into(),
            })
            .await?;

        assert!(
            result.starts_with("[edit_file]"),
            "Output must start with '[edit_file]', got: {}",
            result
        );
        assert!(
            result.contains(file_path.display().to_string().as_str()),
            "Output must contain the absolute file path, got: {}",
            result
        );
        assert!(result.contains("Edited"));
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_near_marker_allowed() -> anyhow::Result<()> {
        // Insert marker before index 9 → marker lands at line 10.
        // Edit on line 8 — within radius of 7.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let mut lines: Vec<String> = (1..=20).map(|i| format!("line {}", i)).collect();
        insert_marker_before(&mut lines, 9); // marker at line 10
        std::fs::write(&file_path, lines.join("\n"))?;

        let tool = make_tool(&file_path);
        tool.call(EditFileArgs {
            old_text: "line 8".into(),
            new_text: "edited".into(),
        })
        .await?;

        let content = std::fs::read_to_string(&file_path)?;
        assert!(content.contains("edited"));
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_far_from_marker_rejected() -> anyhow::Result<()> {
        // Marker inserted before index 19 → marker at line 20.
        // Edit on line 5 — more than 7 lines away.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let mut lines: Vec<String> = (1..=30).map(|i| format!("line {}", i)).collect();
        insert_marker_before(&mut lines, 19); // marker at line 20
        std::fs::write(&file_path, lines.join("\n"))?;

        let tool = make_tool(&file_path);
        let result = tool
            .call(EditFileArgs {
                old_text: "line 5".into(),
                new_text: "hacked".into(),
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("neither the start nor end line"),
            "Expected line-range rejection, got: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&file_path)?.lines().nth(4).unwrap(),
            "line 5"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_multiline_span_allowed() -> anyhow::Result<()> {
        // Multiline [[ ]] marker spanning lines 10-12. Edit on line 9 should be allowed.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let mut lines: Vec<String> = (1..=25).map(|i| format!("line {}", i)).collect();
        // Replace line 10 with opening, add body + close.
        lines[9] = "rik: [[".to_string(); // line 10
        lines.insert(10, "some instruction".to_string()); // line 11
        lines.insert(11, "]]".to_string()); // line 12
        std::fs::write(&file_path, lines.join("\n"))?;

        let tool = make_tool(&file_path);
        tool.call(EditFileArgs {
            old_text: "line 9".into(),
            new_text: "edited".into(),
        })
        .await?;

        let content = std::fs::read_to_string(&file_path)?;
        assert!(content.contains("edited"));
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_rejects_replacing_only_multiline_opener() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let content =
            "// rik: [ uppercase this text\nA lone oak stands.\nDreams of spring.\n// ]\n";
        std::fs::write(&file_path, content)?;

        let tool = make_tool(&file_path);
        let result = tool
            .call(EditFileArgs {
                old_text: "// rik: [ uppercase this text".into(),
                new_text: "A LONE OAK STANDS.".into(),
            })
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must also replace through its closing delimiter")
        );
        assert_eq!(std::fs::read_to_string(&file_path)?, content);
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_allows_replacing_entire_multiline_block() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let block = "// rik: [ uppercase this text\nA lone oak stands.\nDreams of spring.\n// ]";
        std::fs::write(&file_path, format!("{block}\nafter\n"))?;

        let tool = make_tool(&file_path);
        tool.call(EditFileArgs {
            old_text: block.into(),
            new_text: "A LONE OAK STANDS.\nDREAMS OF SPRING.".into(),
        })
        .await?;

        assert_eq!(
            std::fs::read_to_string(&file_path)?,
            "A LONE OAK STANDS.\nDREAMS OF SPRING.\nafter\n"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_near_second_marker_allowed() -> anyhow::Result<()> {
        // Two markers: line 5 and line 26. Edit near line 28 is fine.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let mut lines: Vec<String> = (1..=30).map(|i| format!("line {}", i)).collect();
        insert_marker_before(&mut lines, 4); // marker at line 5
        insert_marker_before(&mut lines, 25); // marker at line 26 (shifted by prior insert)
        std::fs::write(&file_path, lines.join("\n"))?;

        let tool = make_tool(&file_path);
        tool.call(EditFileArgs {
            old_text: "line 28".into(),
            new_text: "edited".into(),
        })
        .await?;

        let content = std::fs::read_to_string(&file_path)?;
        assert!(content.contains("edited"));
        Ok(())
    }

    #[tokio::test]
    async fn test_definition_includes_file_path() -> anyhow::Result<()> {
        let app_state = crate::state::AppState::new(
            std::env::current_dir()?,
            crate::config::Config::default(),
        )?;
        let tool = EditFileTool {
            app_state: &app_state,
            target_path: "src/main.rs".to_string(),
            alias: "rik".to_string(),
            read_history: Arc::default(),
        };
        let def = tool.definition(String::new()).await;
        assert!(def.description.contains("src/main.rs"));
        assert!(!def.description.contains("TARGET_PATH"));
        // file_path must NOT be a parameter
        let params = serde_json::to_string(&def.parameters)?;
        assert!(
            !params.contains("file_path"),
            "file_path should not appear in parameters, got: {params}"
        );
        assert!(params.contains("old_text"));
        assert!(params.contains("new_text"));
        Ok(())
    }

    #[test]
    fn test_byte_offset_to_line() {
        let content = "one\ntwo\nthree\n";
        assert_eq!(byte_offset_to_line(content, 0), 1);
        assert_eq!(byte_offset_to_line(content, 4), 2);
        assert_eq!(byte_offset_to_line(content, 8), 3);
        assert_eq!(byte_offset_to_line(content, 14), 4);
    }

    #[test]
    fn test_byte_offset_to_line_with_multibyte_chars() {
        // 🐸 is 4 bytes (U+1F438).  Use it to ensure byte offsets that land
        // inside a multi-byte character don't panic.
        let content = "use anyhow::Context;\nlet frog = \"🐸\";\n";
        // Line 1 is 20 bytes (+ newline = 21).  Byte 22 starts line 2.
        assert_eq!(byte_offset_to_line(content, 21), 2);
        // Byte 30 lands inside the 🐸 character (bytes 29..33) — must not panic.
        let line = byte_offset_to_line(content, 30);
        assert_eq!(line, 2);
        // Byte far past the end — floors to content.len() which is after the
        // trailing newline, so it counts as line 3 (two '\n' chars seen).
        let line = byte_offset_to_line(content, 999);
        assert_eq!(line, 3);
    }

    /// Build a file with numbered lines and a `rik:` marker at the given line number.
    /// Returns the full file content string.
    fn build_file_with_marker(total_lines: usize, marker_line: usize) -> String {
        let mut parts: Vec<String> = Vec::new();
        for i in 1..=total_lines {
            if i == marker_line {
                parts.push("rik: do something".to_string());
            } else {
                parts.push(format!("line {}", i));
            }
        }
        parts.join("\n")
    }

    #[tokio::test]
    async fn test_wide_edit_spanning_marker_rejected() -> anyhow::Result<()> {
        // Marker on line 10. Edit spans lines 1-20.
        // Neither endpoint (line 1 nor line 20) is within MARKER_RADIUS=7 of line 10.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let content = build_file_with_marker(25, 10);
        std::fs::write(&file_path, &content)?;

        // Grab the exact text from lines 1..=20 from the file.
        let file_lines: Vec<&str> = content.lines().collect();
        let old_text = file_lines[0..20].join("\n");

        let tool = make_tool(&file_path);
        let result = tool
            .call(EditFileArgs {
                old_text,
                new_text: "replaced".into(),
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("neither the start nor end line"),
            "Expected rejection when both endpoints are far from marker, got: {err}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_endpoint_near_marker_allowed() -> anyhow::Result<()> {
        // Marker on line 10. Edit spans lines 3-20.
        // Endpoint Q=line 3 is within [10-7, 10+7]=[3,17], so allowed.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let content = build_file_with_marker(25, 10);
        std::fs::write(&file_path, &content)?;

        let file_lines: Vec<&str> = content.lines().collect();
        let old_text = file_lines[2..20].join("\n");

        let tool = make_tool(&file_path);
        tool.call(EditFileArgs {
            old_text,
            new_text: "replaced".into(),
        })
        .await?;

        let content = std::fs::read_to_string(&file_path)?;
        assert!(content.contains("replaced"));
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_end_point_near_marker_allowed() -> anyhow::Result<()> {
        // Marker on line 10. Edit spans lines 1-17.
        // Start Q=1 is NOT in [3,17], but end P=17 IS in [3,17]. Allowed.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let content = build_file_with_marker(25, 10);
        std::fs::write(&file_path, &content)?;

        let file_lines: Vec<&str> = content.lines().collect();
        let old_text = file_lines[0..17].join("\n");

        let tool = make_tool(&file_path);
        tool.call(EditFileArgs {
            old_text,
            new_text: "replaced".into(),
        })
        .await?;

        let content = std::fs::read_to_string(&file_path)?;
        assert!(content.contains("replaced"));
        Ok(())
    }

    #[tokio::test]
    async fn test_marker_positions_refresh_after_edit() -> anyhow::Result<()> {
        // Two markers; first edit adds lines near marker 1, shifting marker 2.
        // Second edit near marker 2 must still succeed because positions were refreshed.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        // Build file with unique labels so old_text always matches exactly once.
        let mut parts: Vec<String> = Vec::new();
        for i in 1..=20u32 {
            if i == 3 {
                parts.push("rik: do something".to_string());
            } else if i == 15 {
                parts.push("rik: do something".to_string());
            } else {
                parts.push(format!("row_{:02}", i));
            }
        }
        let content = parts.join("\n");
        std::fs::write(&file_path, &content)?;

        let tool = make_tool(&file_path);

        // First edit near marker A (line 3) — replace row_02 with 3 lines.
        tool.call(EditFileArgs {
            old_text: "row_02".into(),
            new_text: "expanded_a\nexpanded_b\nexpanded_c".into(),
        })
        .await?;

        // Marker B was at original line 15, now shifted down by 2.
        // Edit near it — row_14 still exists and is unique.
        tool.call(EditFileArgs {
            old_text: "row_14".into(),
            new_text: "fixed_near_b".into(),
        })
        .await?;

        let content = std::fs::read_to_string(&file_path)?;
        assert!(content.contains("fixed_near_b"));
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_near_marker_with_emoji_content() -> anyhow::Result<()> {
        // File has emoji-heavy content. Marker at line 5. Edit on line 6 (within radius).
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let content = "use anyhow::Context;\n\
            let frog = \"🐸\";\n\
            let sparkles = \"✨✨✨\";\n\
            let family = \"👨\u{200d}👩\u{200d}👧\u{200d}👦\";\n\
            rik: do something\n\
            let rocket = \"🚀\";\n\
            let tree = \"🌲\";\n\
            let waves = \"🌊\";\n";
        std::fs::write(&file_path, content)?;

        let tool = make_tool(&file_path);
        tool.call(EditFileArgs {
            old_text: "let rocket = \"🚀\";".into(),
            new_text: "let rocket = \"🔥\";".into(),
        })
        .await?;

        let result = std::fs::read_to_string(&file_path)?;
        assert!(result.contains("🔥"));
        assert!(!result.contains("🚀"));
        // Other emoji lines untouched
        assert!(result.contains("🐸"));
        assert!(result.contains("👨‍👩‍👧‍👦"));
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_emoji_old_text_near_marker() -> anyhow::Result<()> {
        // The old_text itself contains multi-byte emoji characters.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let content = "line 1\nline 2\nrik: replace the logo\nline 4\nlet logo = \"🐸\";\nline 6\n";
        std::fs::write(&file_path, content)?;

        let tool = make_tool(&file_path);
        tool.call(EditFileArgs {
            old_text: "let logo = \"🐸\";".into(),
            new_text: "let logo = \"🦊\";".into(),
        })
        .await?;

        let result = std::fs::read_to_string(&file_path)?;
        assert!(result.contains("🦊"));
        assert!(!result.contains("🐸"));
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_far_from_marker_with_emoji_rejected() -> anyhow::Result<()> {
        // Emoji content far from the marker — edit must be rejected.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let content = concat!(
            "let a = \"🐸🐸🐸\";\n",
            "let b = \"✨\";\n",
            "let c = \"🌈\";\n",
            "let d = \"🎉\";\n",
            "let e = \"🎃\";\n",
            "let f = \"👽\";\n",
            "let g = \"🤖\";\n",
            "let h = \"🧠\";\n",
            "let i = \"💡\";\n",
            "rik: do something\n",
            "let j = \"🚀\";\n",
        );
        std::fs::write(&file_path, content)?;

        let tool = make_tool(&file_path);
        let result = tool
            .call(EditFileArgs {
                old_text: "let a = \"🐸🐸🐸\";".into(),
                new_text: "let a = \"🐍\";".into(),
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("neither the start nor end line"),
            "Expected rejection, got: {err}"
        );
        // File must be unchanged
        let content = std::fs::read_to_string(&file_path)?;
        assert!(content.contains("🐸🐸🐸"));
        Ok(())
    }
}
