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
    /// Path of the file to edit.
    pub file_path: String,
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

/// A tool that performs exact text replacement in an existing file.
///
/// Finds `old_text` in the file and replaces it with `new_text`.
/// The `old_text` must match exactly one occurrence -- zero or multiple matches
/// are errors.
///
/// Two restrictions are enforced:
/// - **File scope**: only `allowed_path` may be edited.
/// - **Line scope**: the edit must fall within `MARKER_RADIUS` lines of a
///   marker span recorded in `marker_spans`.
#[derive(Deserialize, Serialize)]
pub struct EditFileTool {
    /// Only this file path may be edited.
    pub allowed_path: String,
    /// Marker spans as `(1-based start line, 1-based end line)` tuples.
    /// An edit is allowed when its line range overlaps with at least one
    /// expanded span `[start - MARKER_RADIUS, end + MARKER_RADIUS]`.
    pub marker_spans: Vec<(usize, usize)>,
}

impl Tool for EditFileTool {
    const NAME: &'static str = "edit_file";

    type Error = EditFileError;
    type Args = EditFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Edit the target file by replacing exact text. \
                          old_text must match exactly one occurrence. \
                          That text is replaced with new_text. \
                          Edits are restricted to the target file only, \
                          and must be within 7 lines of a marker."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path of the file to edit (must be the target file)"
                    },
                    "old_text": {
                        "type": "string",
                        "description": "Exact text to find in the file (must be unique)"
                    },
                    "new_text": {
                        "type": "string",
                        "description": "Replacement text"
                    }
                },
                "required": ["file_path", "old_text", "new_text"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = Path::new(&args.file_path);

        if !path.exists() {
            return Err(EditFileError(format!("File not found: {}", args.file_path)));
        }

        // --- File scope restriction ---
        let canonical_requested = std::fs::canonicalize(path)
            .map_err(|e| EditFileError(format!("Cannot resolve path: {e}")))?;
        let canonical_allowed = Path::new(&self.allowed_path).canonicalize()
            .map_err(|e| EditFileError(format!("Cannot resolve allowed path: {e}")))?;
        if canonical_requested != canonical_allowed {
            return Err(EditFileError(format!(
                "Edit rejected: edits are only allowed in {}. Got: {}",
                canonical_allowed.display(),
                canonical_requested.display(),
            )));
        }

        let content = std::fs::read_to_string(path)?;

        let first = content.find(&args.old_text);
        let last = content.rfind(&args.old_text);

        match (first, last) {
            (None, _) => Err(EditFileError(format!(
                "old_text not found in file: {}",
                args.file_path
            ))),
            (Some(a), Some(b)) if a != b => Err(EditFileError(format!(
                "old_text matches {} locations in file -- must be unique",
                content.matches(&args.old_text).count()
            ))),
            (Some(pos), _) => {
                // --- Line scope restriction ---
                if !self.is_edit_near_marker(&content, pos, &args.old_text) {
                    return Err(EditFileError(
                        "Edit rejected: old_text is not within 7 lines of any marker. \
                         Edits must be close to a marker line."
                            .to_string(),
                    ));
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
                    args.file_path,
                    args.old_text.len(),
                    args.new_text.len(),
                    args.file_path
                ))
            }
        }
    }
}

impl EditFileTool {
    /// Check whether the edit starting at byte offset `pos` with text `old_text`
    /// falls within `MARKER_RADIUS` lines of at least one marker span.
    fn is_edit_near_marker(&self, content: &str, pos: usize, old_text: &str) -> bool {
        // Compute 1-based line number for the start of old_text.
        let edit_start_line = byte_offset_to_line(content, pos);
        // Compute 1-based line number for the end of old_text.
        let edit_end_line = byte_offset_to_line(content, pos + old_text.len());

        for &(start, end) in &self.marker_spans {
            let lo = start.saturating_sub(MARKER_RADIUS).max(1);
            let hi = end + MARKER_RADIUS;
            // Edit range overlaps with expanded marker range?
            if edit_start_line <= hi && edit_end_line >= lo {
                return true;
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

    /// Helper to build a tool with the same file as allowed path and a single
    /// marker span around `marker_line`.
    fn make_tool(file_path: &std::path::Path, marker_line: usize) -> EditFileTool {
        EditFileTool {
            allowed_path: file_path.display().to_string(),
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
            file_path: file_path.display().to_string(),
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
            allowed_path: "/nonexistent".to_string(),
            marker_spans: vec![(1, 1)],
        };
        let result = tool
            .call(EditFileArgs {
                file_path: "/nonexistent".to_string(),
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
                file_path: file_path.display().to_string(),
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
                file_path: file_path.display().to_string(),
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
                file_path: file_path.display().to_string(),
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
    async fn test_edit_file_rejected_wrong_path() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let allowed_file = dir.path().join("allowed.txt");
        let other_file = dir.path().join("other.txt");
        std::fs::write(&allowed_file, "allowed content\n")?;
        std::fs::write(&other_file, "other content\n")?;

        let tool = EditFileTool {
            allowed_path: allowed_file.display().to_string(),
            marker_spans: vec![(1, 1)],
        };
        let result = tool
            .call(EditFileArgs {
                file_path: other_file.display().to_string(),
                old_text: "other content".into(),
                new_text: "hacked".into(),
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Edit rejected"), "Expected 'Edit rejected', got: {err}");
        assert_eq!(std::fs::read_to_string(&other_file)?, "other content\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_edit_file_allowed_path_permitted() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let allowed_file = dir.path().join("allowed.txt");
        std::fs::write(&allowed_file, "hello world\n")?;

        let tool = EditFileTool {
            allowed_path: allowed_file.display().to_string(),
            marker_spans: vec![(1, 1)],
        };
        tool.call(EditFileArgs {
            file_path: allowed_file.display().to_string(),
            old_text: "hello world".into(),
            new_text: "goodbye".into(),
        })
        .await?;

        assert_eq!(std::fs::read_to_string(&allowed_file)?, "goodbye\n");
        Ok(())
    }

    // -------------------------------------------------------------------
    // Line-range restriction tests
    // -------------------------------------------------------------------

    #[tokio::test]
    async fn test_edit_near_marker_allowed() -> anyhow::Result<()> {
        // Marker on line 10, edit on line 8 — within radius of 7.
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        std::fs::write(&file_path, lines.join("\n"))?;

        let tool = EditFileTool {
            allowed_path: file_path.display().to_string(),
            marker_spans: vec![(10, 10)],
        };
        tool.call(EditFileArgs {
            file_path: file_path.display().to_string(),
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
            allowed_path: file_path.display().to_string(),
            marker_spans: vec![(20, 20)],
        };
        let result = tool
            .call(EditFileArgs {
                file_path: file_path.display().to_string(),
                old_text: "line 5".into(),
                new_text: "hacked".into(),
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not within 7 lines of any marker"),
            "Expected line-range rejection, got: {err}"
        );
        assert_eq!(std::fs::read_to_string(&file_path)?.lines().nth(4).unwrap(), "line 5");
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
            allowed_path: file_path.display().to_string(),
            marker_spans: vec![(10, 15)],
        };
        tool.call(EditFileArgs {
            file_path: file_path.display().to_string(),
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
            allowed_path: file_path.display().to_string(),
            marker_spans: vec![(5, 5), (25, 25)],
        };
        tool.call(EditFileArgs {
            file_path: file_path.display().to_string(),
            old_text: "line 28".into(),
            new_text: "edited".into(),
        })
        .await?;

        let content = std::fs::read_to_string(&file_path)?;
        assert!(content.contains("edited"));
        Ok(())
    }

    #[test]
    fn test_byte_offset_to_line() {
        let content = "one\ntwo\nthree\n";
        assert_eq!(byte_offset_to_line(content, 0), 1);  // "o" in "one"
        assert_eq!(byte_offset_to_line(content, 4), 2);  // "t" in "two"
        assert_eq!(byte_offset_to_line(content, 8), 3);  // "t" in "three"
        assert_eq!(byte_offset_to_line(content, 14), 4); // past end of last line
    }
}
