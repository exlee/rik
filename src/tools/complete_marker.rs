use std::path::Path;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

// ---------------------------------------------------------------------------
// CompleteMarker tool
// ---------------------------------------------------------------------------

/// Arguments for the complete_marker tool.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct CompleteMarkerArgs {
    /// Path of the file containing the marker.
    pub file_path: String,
    /// The completed text to replace the marker with.
    pub completed_text: String,
}

/// Error type for the complete_marker tool.
#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CompleteMarkerError(String);

impl From<std::io::Error> for CompleteMarkerError {
    fn from(e: std::io::Error) -> Self {
        CompleteMarkerError(e.to_string())
    }
}

#[allow(dead_code)]
/// A tool that replaces a `rik: <query>` marker in a file with completed text.
///
/// This is the **only** writing tool available in file-completion mode, ensuring
/// edits are restricted to the marker location.
#[derive(Deserialize, Serialize)]
pub struct CompleteMarkerTool;

impl Tool for CompleteMarkerTool {
    const NAME: &'static str = "complete_marker";

    type Error = CompleteMarkerError;
    type Args = CompleteMarkerArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Replace the entire line containing `rik: <query>` with the \",
                          completed text. This is the ONLY way to write output in \
                          file-completion mode."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path of the file containing the marker to replace"
                    },
                    "completed_text": {
                        "type": "string",
                        "description": "The completed text to write in place of the marker line"
                    }
                },
                "required": ["file_path", "completed_text"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = Path::new(&args.file_path);
        let content = std::fs::read_to_string(path)?;

        let replaced = replace_marker(&content, "rik", &args.completed_text).ok_or_else(|| {
            CompleteMarkerError(format!(
                "No 'rik:' marker found in file: {}",
                args.file_path
            ))
        })?;

        std::fs::write(path, replaced)?;
        Ok(format!(
            "[complete_marker] path={} content={}\nReplaced marker in {}",
            args.file_path, args.completed_text, args.file_path
        ))
    }
}

/// Replace the first matching marker in `content` with `replacement`.
///
/// The **entire line** containing the marker is removed. The replacement text is
/// inserted in its place. Surrounding lines are preserved unchanged.
#[allow(dead_code)]
pub fn replace_marker(content: &str, alias: &str, replacement: &str) -> Option<String> {
    let prefix = format!("{alias}:");
    let mut result_lines: Vec<String> = Vec::new();
    let mut found = false;

    for line in content.lines() {
        if !found && line.contains(&prefix) {
            for rline in replacement.lines() {
                result_lines.push(rline.to_string());
            }
            found = true;
            continue;
        }
        result_lines.push(line.to_string());
    }

    if !found {
        return None;
    }

    let mut result = result_lines.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    Some(result)
}

/// Opening delimiters that start a multi-line marker, sorted longest-first.
/// Each entry is (open, close) pair.
const MULTILINE_DELIMITERS: &[(&str, &str)] = &[
    ("[[[", "]]]"),
    ("(((", ")))"),
    ("{{{", "}}}"),
    ("[[", "]]"),
    ("((", "))"),
    ("{{", "}}"),
    ("[", "]"),
    ("(", ")"),
    ("{", "}"),
];

/// Check if text after `alias:` is a multi-line opening delimiter.
/// Returns (open, close) pair if it is.
fn match_opening_delimiter(text: &str) -> Option<(&'static str, &'static str)> {
    let trimmed = text.trim();
    // Match longest first to avoid e.g. [[[ matching as a single [
    for &(open, close) in MULTILINE_DELIMITERS {
        if trimmed == open {
            return Some((open, close));
        }
    }
    None
}

/// Check if a line is the closing delimiter for a multi-line block.
/// The line should be exactly `alias: close` (with optional whitespace).
fn is_closing_line(line: &str, alias: &str, close: &str) -> bool {
    let prefix = format!("{alias}:");
    if let Some(pos) = line.find(&prefix) {
        let after = line[pos + prefix.len()..].trim();
        // Allow close to have leading whitespace stripped by the trim
        after == close || after == close.trim()
    } else {
        false
    }
}

/// Find all markers matching the given alias prefix in content.
///
/// Single-line: `{alias}: <query>` — everything after the prefix on the same line.
///
/// Multi-line: `{alias}: <open>` starts a block, `{alias}: <close>` ends it.
/// Supported delimiter pairs: `[`/`]`, `[[`/`]]`, `[[[`/`]]]`,
/// `(`/`)`, `((`/`))`, `(((`/`)))`, `{`/`}`, `{{`/`}}`, `{{{`/`}}}`.
/// Content between open and close is the query, with leading whitespace stripped
/// per line and blank lines preserved.
///
/// Returns `Vec<(start_line, end_line, query)>` where line numbers are 1-based.
/// For single-line markers `start_line == end_line`. For multi-line markers
/// `end_line` is the closing delimiter line.
pub fn find_markers(content: &str, alias: &str) -> Vec<(usize, usize, String)> {
    let prefix = format!("{alias}:");
    let lines: Vec<&str> = content.lines().collect();
    let mut markers = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        if let Some(pos) = line.find(&prefix) {
            let after = line[pos + prefix.len()..].trim();

            if let Some((_open, close)) = match_opening_delimiter(after) {
                // Multi-line marker: collect lines until closing delimiter
                let start_line = i + 1; // 1-based
                let mut inner_lines: Vec<String> = Vec::new();
                let mut found_close = false;

                let mut j = i + 1;
                while j < lines.len() {
                    if is_closing_line(lines[j], alias, close) {
                        found_close = true;
                        break;
                    }
                    // Strip leading whitespace from inner lines
                    inner_lines.push(lines[j].trim_start().to_string());
                    j += 1;
                }

                if found_close && !inner_lines.is_empty() {
                    let end_line = j + 1; // 1-based, closing delimiter line
                    markers.push((start_line, end_line, inner_lines.join("\n")));
                    i = j + 1; // skip past closing line
                    continue;
                } else {
                    // Mismatched/unclosed delimiter — treat opening line as single-line marker
                    markers.push((start_line, start_line, after.to_string()));
                    i += 1;
                    continue;
                }
            } else if !after.is_empty() {
                // Single-line marker
                markers.push((i + 1, i + 1, after.to_string()));
            }
        }
        i += 1;
    }

    markers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_replace_marker_line_prefix() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "before\nrik: write a poem\nafter\n")?;

        let tool = CompleteMarkerTool;
        tool.call(CompleteMarkerArgs {
            file_path: file_path.display().to_string(),
            completed_text: "Roses are red\nViolets are blue".into(),
        })
        .await?;

        let content = std::fs::read_to_string(&file_path)?;
        assert_eq!(content, "before\nRoses are red\nViolets are blue\nafter\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_replace_marker_in_brackets() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "summary: [rik: describe the module]\n")?;

        let tool = CompleteMarkerTool;
        tool.call(CompleteMarkerArgs {
            file_path: file_path.display().to_string(),
            completed_text: "handles auth and permissions".into(),
        })
        .await?;

        // Entire line is replaced
        let content = std::fs::read_to_string(&file_path)?;
        assert_eq!(content, "handles auth and permissions\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_replace_marker_in_braces() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "let x = {rik: generate value};\n")?;

        let tool = CompleteMarkerTool;
        tool.call(CompleteMarkerArgs {
            file_path: file_path.display().to_string(),
            completed_text: "42".into(),
        })
        .await?;

        let content = std::fs::read_to_string(&file_path)?;
        assert_eq!(content, "42\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_replace_marker_mid_line() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "const desc = \"rik: fill in\";\n")?;

        let tool = CompleteMarkerTool;
        tool.call(CompleteMarkerArgs {
            file_path: file_path.display().to_string(),
            completed_text: "a placeholder".into(),
        })
        .await?;

        let content = std::fs::read_to_string(&file_path)?;
        assert_eq!(content, "a placeholder\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_replace_no_marker() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "no marker here\n").unwrap();

        let tool = CompleteMarkerTool;
        let result = tool
            .call(CompleteMarkerArgs {
                file_path: file_path.display().to_string(),
                completed_text: "result".into(),
            })
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_find_markers_various_positions() {
        let content = "rik: first query\n\
                       world\n\
                       [rik: second query]\n\
                       rik:\n\
                       {rik: third}\n\
                       const x = \"rik: fourth\";\n\
                       end";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 4);
        assert_eq!(markers[0], (1, 1, "first query".to_string()));
        assert_eq!(markers[1], (3, 3, "second query]".to_string()));
        assert_eq!(markers[2], (5, 5, "third}".to_string()));
        assert_eq!(markers[3], (6, 6, "fourth\";".to_string()));
    }

    #[test]
    fn test_find_markers_empty_query_ignored() {
        let content = "rik:\nsome text\nrik:   \n";
        let markers = find_markers(content, "rik");
        assert!(markers.is_empty());
    }

    #[tokio::test]
    async fn test_complete_marker_prints_tool_name_and_params() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "before\nrik: write a poem\nafter\n")?;

        let completed_text = "Roses are red\nViolets are blue";

        let tool = CompleteMarkerTool;
        let result = tool
            .call(CompleteMarkerArgs {
                file_path: file_path.display().to_string(),
                completed_text: completed_text.into(),
            })
            .await?;

        assert!(
            result.starts_with("[complete_marker]"),
            "Output must start with '[complete_marker]', got: {}",
            result
        );
        assert!(
            result.contains(file_path.display().to_string().as_str()),
            "Output must contain the file path, got: {}",
            result
        );
        assert!(
            result.contains(completed_text),
            "Output must contain the completed text, got: {}",
            result
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Multiline instruction parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_find_markers_multiline_double_brackets() {
        let content = "before\nrik: [[\nA\nB\nC\nrik: ]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 6, "A\nB\nC".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_parens() {
        let content = "before\nrik: (\nA\nB\nC\nrik: )\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 6, "A\nB\nC".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_curly_braces() {
        let content = "before\nrik: {{\nA\nB\nC\nrik: }}\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 6, "A\nB\nC".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_triple_brackets() {
        let content = "before\nrik: [[[\nline1\nline2\nrik: ]]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 5, "line1\nline2".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_triple_parens() {
        let content = "before\nrik: (((\nfoo\nbar\nbaz\nrik: )))\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 6, "foo\nbar\nbaz".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_triple_curly() {
        let content = "before\nrik: {{{\nhello\nworld\nrik: }}}\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 5, "hello\nworld".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_single_bracket() {
        let content = "before\nrik: [\nsingle line inside\nrik: ]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 4, "single line inside".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_single_paren() {
        let content = "before\nrik: (\nsingle paren content\nrik:)\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 4, "single paren content".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_single_curly() {
        let content = "before\nrik: {\ncurly content here\nrik: }\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 4, "curly content here".to_string()));
    }

    #[test]
    fn test_find_markers_mismatched_delimiters_not_multiline() {
        // Open with [[ close with )) - mismatch, should not match as multiline
        let content = "rik: [[\nmismatched\nrik: ))";
        let markers = find_markers(content, "rik");
        // Should fall back to single-line behavior or not match multiline
        assert!(!markers.is_empty());
    }

    #[test]
    fn test_find_markers_multiple_multiline_in_file() {
        let content = "start\nrik: [[\nfirst A\nfirst B\nrik: ]]\nmiddle\nrik: ((\nsecond X\nsecond Y\nrik: ))\nend";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0], (2, 5, "first A\nfirst B".to_string()));
        assert_eq!(markers[1], (7, 10, "second X\nsecond Y".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_with_leading_whitespace() {
        let content = "before\nrik: [[\n  indented A\n  indented B\nrik: ]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 5, "indented A\nindented B".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_preserves_blank_lines_inside() {
        let content = "before\nrik: [[\nA\n\nC\nrik: ]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 6, "A\n\nC".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_closing_on_separate_line_only_alias_prefix() {
        // The closing delimiter line contains only alias and closing bracket
        let content = "before\nrik: [[\ncontent line\nrik: ]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 4, "content line".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_surrounding_context_preserved() {
        let content = "line before\nrik: [[\ninstruction body\nrik: ]]\nline after";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 4, "instruction body".to_string()));
    }

    #[test]
    fn test_find_markers_single_and_multiline_mixed() {
        let content = "rik: simple query\nrik: [[\nmulti\nline\nrik: ]]\nrik: another simple";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 3);
        assert_eq!(markers[0], (1, 1, "simple query".to_string()));
        assert_eq!(markers[1], (2, 5, "multi\nline".to_string()));
        assert_eq!(markers[2], (6, 6, "another simple".to_string()));
    }
}
