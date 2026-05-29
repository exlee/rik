use std::path::Path;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

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
///   marker span recorded in `marker_spans`.
#[derive(Deserialize, Serialize)]
pub struct EditFileTool {
    /// The file this tool is allowed to edit (set at construction time).
    pub target_path: String,
    /// Marker spans as `(1-based start line, 1-based end line)` tuples.
    /// An edit is allowed when either its start line or end line falls
    /// within `MARKER_RADIUS` lines of any line inside a marker span.
    pub marker_spans: Vec<(usize, usize)>,
}

impl Tool for EditFileTool {
    const NAME: &'static str = "edit_file";

    type Error = EditFileError;
    type Args = EditFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        let desc = format!(
            "Edit {} by replacing exact text. \
             old_text must match exactly one occurrence in the file. \
             That text is replaced with new_text. \
             Only this file may be edited and edits must be within {} lines of a marker.",
            self.target_path, MARKER_RADIUS,
        );
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
        let path = Path::new(&self.target_path);

        if !path.exists() {
            return Err(EditFileError(format!(
                "File not found: {}",
                self.target_path
            )));
        }

        let content = std::fs::read_to_string(path)?;

        let first = content.find(&args.old_text);
        let last = content.rfind(&args.old_text);

        match (first, last) {
            (None, _) => Err(EditFileError(format!(
                "old_text not found in file: {}",
                self.target_path
            ))),
            (Some(a), Some(b)) if a != b => Err(EditFileError(format!(
                "old_text matches {} locations in file -- must be unique",
                content.matches(&args.old_text).count()
            ))),
            (Some(pos), _) => {
                // --- Line scope restriction ---
                if !self.is_edit_near_marker(&content, pos, &args.old_text) {
                    return Err(EditFileError(format!(
                        "Edit rejected: neither the start nor end line of old_text \
                         is within {} lines of any marker. \
                         Edits must anchor near a marker line.",
                        MARKER_RADIUS,
                    )));
                }

                let mut new_content = String::with_capacity(
                    content.len() - args.old_text.len() + args.new_text.len(),
                );
                new_content.push_str(&content[..pos]);
                new_content.push_str(&args.new_text);
                new_content.push_str(&content[pos + args.old_text.len()..]);

                std::fs::write(path, &new_content)?;
                Ok(format!(
                    "[edit_file] path={} input_len={} output_len={}\nEdited {}",
                    self.target_path,
                    args.old_text.len(),
                    args.new_text.len(),
                    self.target_path
                ))
            }
        }
    }
}

impl EditFileTool {
    /// Check whether the edit's start or end line falls within `MARKER_RADIUS`
    /// lines of any line inside at least one marker span.
    ///
    /// This mirrors the Prolog rule:
    ///   possible(edit(Q,P), marker(X,Y)) :-
    ///       between(X,Y,Z), (in_range(Z,Q) ; in_range(Z,P)).
    /// where `in_range(Z, V)` means V ∈ [Z − max_offset, Z + max_offset].
    fn is_edit_near_marker(&self, content: &str, pos: usize, old_text: &str) -> bool {
        let edit_start_line = byte_offset_to_line(content, pos);
        // Line of the last character of the matched text.
        let edit_end_line = byte_offset_to_line(content, pos + old_text.len().saturating_sub(1));

        for &(start, end) in &self.marker_spans {
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
}

/// Convert a byte offset in `content` to a 1-based line number.
fn byte_offset_to_line(content: &str, offset: usize) -> usize {
    let offset = offset.min(content.len());
    content[..offset]
        .chars()
        .filter(|&c| c == '\n')
        .count() as usize
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a tool targeting `file_path` with a single marker span.
    fn make_tool(file_path: &std::path::Path, marker_line: usize) -> EditFileTool {
        EditFileTool {
            target_path: file_path.display().to_string(),
            marker_spans: vec![(marker_line, marker_line)],
        }
    }

    #[tokio::test]
    async fn test_edit_file_replace_line() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "before\nrik: write a poem\nafter\n")?;

        let tool = make_tool(&file_path, 2);
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
    async fn test_edit_file_not_found() {
        let tool = EditFileTool {
            target_path: "/nonexistent".to_string(),
            marker_spans: vec![(1, 1)],
        };
        let result = tool
            .call(EditFileArgs {
                old_text: "x".into(),
                new_text: "y".into(),
            })
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("File not found"));
    }

    #[tokio::test]
    async fn test_edit_file_old_text_missing() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world\n")?;

        let tool = make_tool(&file_path, 1);
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
        std::fs::write(&file_path, "abc xyz abc\n")?;

        let tool = make_tool(&file_path, 1);
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
        std::fs::write(&file_path, "foo bar baz\n")?;

        let old_text = "bar";
        let new_text = "qux";

        let tool = make_tool(&file_path, 1);
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
            "Output must contain the file path, got: {}",
            result
        );
        assert!(
            result.contains(&format!("input_len={}", old_text.len())),
            "Output must contain 'input_len={} ', got: {}",
            old_text.len(),
            result
        );
        assert!(
            result.contains(&format!("output_len={}", new_text.len())),
            "Output must contain 'output_len={} ', got: {}",
            new_text.len(),
            result
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_near_marker_allowed() -> anyhow::Result<()> {
        // Marker on line 10, edit on line 8 — within radius of 7.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        std::fs::write(&file_path, lines.join("\n"))?;

        let tool = EditFileTool {
            target_path: file_path.display().to_string(),
            marker_spans: vec![(10, 10)],
        };
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
        // Marker on line 20, edit on line 5 — more than 7 lines away.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let lines: Vec<String> = (1..=30).map(|i| format!("line {i}")).collect();
        std::fs::write(&file_path, lines.join("\n"))?;

        let tool = EditFileTool {
            target_path: file_path.display().to_string(),
            marker_spans: vec![(20, 20)],
        };
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
        // Multiline marker spans lines 10-15. Edit on line 9 should be allowed.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let lines: Vec<String> = (1..=25).map(|i| format!("line {i}")).collect();
        std::fs::write(&file_path, lines.join("\n"))?;

        let tool = EditFileTool {
            target_path: file_path.display().to_string(),
            marker_spans: vec![(10, 15)],
        };
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
    async fn test_edit_near_second_marker_allowed() -> anyhow::Result<()> {
        // Two markers: line 5 and line 25. Edit near line 25 is fine even
        // though it's far from line 5.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let lines: Vec<String> = (1..=30).map(|i| format!("line {i}")).collect();
        std::fs::write(&file_path, lines.join("\n"))?;

        let tool = EditFileTool {
            target_path: file_path.display().to_string(),
            marker_spans: vec![(5, 5), (25, 25)],
        };
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
        let tool = EditFileTool {
            target_path: "src/main.rs".to_string(),
            marker_spans: vec![(10, 10)],
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

    #[tokio::test]
    async fn test_wide_edit_spanning_marker_rejected() -> anyhow::Result<()> {
        // Marker on line 10. Edit spans lines 1-20.
        // The middle of the edit crosses the marker, but neither endpoint
        // (line 1 nor line 20) is within MARKER_RADIUS=7 of line 10.
        // Old overlap logic would allow this; Prolog rule rejects it.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let lines: Vec<String> = (1..=25).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&file_path, lines.join(""))?;

        let tool = EditFileTool {
            target_path: file_path.display().to_string(),
            marker_spans: vec![(10, 10)],
        };
        let result = tool
            .call(EditFileArgs {
                old_text: "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11\nline 12\nline 13\nline 14\nline 15\nline 16\nline 17\nline 18\nline 19\nline 20\n".into(),
                new_text: "replaced\n".into(),
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
        let lines: Vec<String> = (1..=25).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&file_path, lines.join(""))?;

        let tool = EditFileTool {
            target_path: file_path.display().to_string(),
            marker_spans: vec![(10, 10)],
        };
        tool.call(EditFileArgs {
            old_text: "line 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11\nline 12\nline 13\nline 14\nline 15\nline 16\nline 17\nline 18\nline 19\nline 20\n".into(),
            new_text: "replaced\n".into(),
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
        let lines: Vec<String> = (1..=25).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&file_path, lines.join(""))?;

        let tool = EditFileTool {
            target_path: file_path.display().to_string(),
            marker_spans: vec![(10, 10)],
        };
        tool.call(EditFileArgs {
            old_text: "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11\nline 12\nline 13\nline 14\nline 15\nline 16\nline 17\n".into(),
            new_text: "replaced\n".into(),
        })
        .await?;

        let content = std::fs::read_to_string(&file_path)?;
        assert!(content.contains("replaced"));
        Ok(())
    }
}
