use dashmap::DashMap;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt::Write;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// ReadFile tool
// ---------------------------------------------------------------------------

/// Arguments for the read_file tool.
#[derive(Deserialize)]
pub struct ReadFileArgs {
    /// Path of the file to read.
    pub path: String,
    /// Optional 1-based line number to start reading from.
    pub offset: Option<usize>,
    /// Optional maximum number of lines to return.
    pub limit: Option<usize>,
}

/// Error type for the read_file tool.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ReadFileError(String);

impl From<std::io::Error> for ReadFileError {
    fn from(e: std::io::Error) -> Self {
        ReadFileError(e.to_string())
    }
}

/// A tool that reads the contents of an existing file.
#[derive(Default)]
struct ReadHistory {
    content_hash: u64,
    ranges: Vec<(usize, usize)>,
}

#[derive(Default)]
pub struct ReadFileHistory {
    files: DashMap<PathBuf, ReadHistory>,
}

impl ReadFileHistory {
    pub fn clear(&self) {
        self.files.clear();
    }
}

#[derive(Deserialize, Serialize)]
pub struct ReadFileTool<'a> {
    #[serde(skip, default = "crate::state::get")]
    pub app_state: &'a crate::state::AppState,
    #[serde(skip, default)]
    pub read_history: Arc<ReadFileHistory>,
}

impl Default for ReadFileTool<'static> {
    fn default() -> Self {
        Self {
            app_state: crate::state::get(),
            read_history: Arc::default(),
        }
    }
}

impl<'a> ReadFileTool<'a> {
    pub fn with_history(
        app_state: &'a crate::state::AppState,
        read_history: Arc<ReadFileHistory>,
    ) -> Self {
        Self {
            app_state,
            read_history,
        }
    }

    #[cfg(test)]
    fn new(app_state: &'a crate::state::AppState) -> Self {
        Self::with_history(app_state, Arc::default())
    }
}

fn unread_ranges(requested: (usize, usize), known: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let (mut cursor, end) = requested;
    let mut unread = Vec::new();

    for &(known_start, known_end) in known {
        if known_end <= cursor {
            continue;
        }
        if known_start >= end {
            break;
        }
        if known_start > cursor {
            unread.push((cursor, known_start.min(end)));
        }
        cursor = cursor.max(known_end);
        if cursor >= end {
            break;
        }
    }

    if cursor < end {
        unread.push((cursor, end));
    }
    unread
}

fn remember_range(known: &mut Vec<(usize, usize)>, range: (usize, usize)) {
    known.push(range);
    known.sort_unstable_by_key(|&(start, _)| start);

    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(known.len());
    for &(start, end) in known.iter() {
        if let Some((_, previous_end)) = merged.last_mut()
            && start <= *previous_end
        {
            *previous_end = (*previous_end).max(end);
        } else {
            merged.push((start, end));
        }
    }
    *known = merged;
}

fn format_ranges(
    path: &Path,
    args: &ReadFileArgs,
    lines: &[&str],
    requested: (usize, usize),
    ranges: &[(usize, usize)],
) -> String {
    let mut header = format!("[read_file] path={}", path.display());
    if let Some(offset) = args.offset {
        write!(header, " offset={offset}").ok();
    }
    if let Some(limit) = args.limit {
        write!(header, " limit={limit}").ok();
    }

    if ranges == [requested] {
        let (start, end) = ranges[0];
        return format!("{header}\n{}", lines[start..end].join("\n"));
    }

    let chunks = ranges
        .iter()
        .map(|&(start, end)| {
            format!(
                "[lines {}-{}]\n{}",
                start + 1,
                end,
                lines[start..end].join("\n")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{header}\n{chunks}")
}

impl Tool for ReadFileTool<'_> {
    const NAME: &'static str = "read_file";

    type Error = ReadFileError;
    type Args = ReadFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read the contents of an existing file. \
                          Optionally specify offset (1-based line number) and limit \
                          to read only a portion of the file. Lines already returned \
                          by this tool are omitted from later reads."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path of the file to read"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "1-based line number to start reading from (optional)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to return (optional)"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = self
            .app_state
            .resolve_path(&args.path)
            .map_err(|e| ReadFileError(e.to_string()))?;

        if !path.exists() {
            return Err(ReadFileError(format!("File not found: {}", path.display())));
        }

        let content = std::fs::read_to_string(&path)?;
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        let content_hash = hasher.finish();

        let lines: Vec<&str> = content.lines().collect();

        let start = args.offset.unwrap_or(1).saturating_sub(1);
        let end = if let Some(limit) = args.limit {
            start.saturating_add(limit).min(lines.len())
        } else {
            lines.len()
        };

        if start >= lines.len() {
            return Ok(String::new());
        }

        let unread = {
            let mut known = self.read_history.files.entry(path.clone()).or_default();
            if known.content_hash != content_hash {
                known.content_hash = content_hash;
                known.ranges.clear();
            }
            let unread = unread_ranges((start, end), &known.ranges);
            for &range in &unread {
                remember_range(&mut known.ranges, range);
            }
            unread
        };

        if unread.is_empty() {
            return Err(ReadFileError("Context known".to_string()));
        }

        Ok(format_ranges(&path, &args, &lines, (start, end), &unread))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_state() -> crate::state::AppState {
        crate::state::AppState::new(
            std::env::current_dir().unwrap(),
            crate::config::Config::default(),
        )
        .unwrap()
    }

    /// Create a subdirectory under cwd and return its relative path.
    /// Caller is responsible for cleaning up.
    fn make_relative_dir(name: &str) -> (std::path::PathBuf, String) {
        let rel = std::path::PathBuf::from(format!(".rik_test_{}", name));
        let abs = std::env::current_dir().unwrap().join(&rel);
        std::fs::create_dir_all(&abs).ok();
        (abs, rel.to_string_lossy().to_string())
    }

    fn cleanup_rel(rel: &str) {
        let p = std::path::PathBuf::from(rel);
        if p.exists() {
            std::fs::remove_dir_all(&p).ok();
        }
    }

    #[tokio::test]
    async fn test_read_file_full() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("full");
        let file_path = abs.join("test.txt");
        let rel_file = format!("{}/test.txt", rel);
        std::fs::write(&file_path, "line1\nline2\nline3")?;

        let app_state = app_state();
        let tool = ReadFileTool::new(&app_state);
        let result = tool
            .call(ReadFileArgs {
                path: rel_file.clone(),
                offset: None,
                limit: None,
            })
            .await?;

        assert_eq!(
            result,
            format!(
                "[read_file] path={}\nline1\nline2\nline3",
                file_path.display()
            )
        );
        cleanup_rel(&rel);
        Ok(())
    }

    #[tokio::test]
    async fn test_read_file_slice() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("slice");
        let file_path = abs.join("test.txt");
        let rel_file = format!("{}/test.txt", rel);
        std::fs::write(&file_path, "line1\nline2\nline3\nline4\nline5")?;

        let app_state = app_state();
        let tool = ReadFileTool::new(&app_state);
        let result = tool
            .call(ReadFileArgs {
                path: rel_file.clone(),
                offset: Some(2),
                limit: Some(2),
            })
            .await?;

        assert!(
            result.ends_with("line2\nline3"),
            "Expected result to end with 'line2\nline3', got: {result}"
        );
        cleanup_rel(&rel);
        Ok(())
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let app_state = app_state();
        let tool = ReadFileTool::new(&app_state);
        let result = tool
            .call(ReadFileArgs {
                path: ".rik_test_nonexistent/file.txt".to_string(),
                offset: None,
                limit: None,
            })
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("File not found"));
    }

    #[tokio::test]
    async fn test_read_file_rejects_absolute_path() {
        let app_state = app_state();
        let tool = ReadFileTool::new(&app_state);
        let result = tool
            .call(ReadFileArgs {
                path: "/etc/hosts".to_string(),
                offset: None,
                limit: None,
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
    async fn test_read_file_rejects_path_traversal() {
        let app_state = app_state();
        let tool = ReadFileTool::new(&app_state);
        let result = tool
            .call(ReadFileArgs {
                path: "../../etc/passwd".to_string(),
                offset: None,
                limit: None,
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("outside watched directory"),
            "Expected path traversal rejection, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_read_file_prints_tool_name_and_params() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("params");
        let file_path = abs.join("test.txt");
        let rel_file = format!("{}/test.txt", rel);
        std::fs::write(&file_path, "line1\nline2")?;

        let app_state = app_state();
        let tool = ReadFileTool::new(&app_state);
        let result = tool
            .call(ReadFileArgs {
                path: rel_file.clone(),
                offset: None,
                limit: None,
            })
            .await?;

        assert!(
            result.starts_with("[read_file]"),
            "Output must start with '[read_file]', got: {}",
            result
        );
        assert!(
            result.contains(&rel_file),
            "Output must contain the file path, got: {}",
            result
        );
        cleanup_rel(&rel);
        Ok(())
    }

    #[tokio::test]
    async fn test_read_file_prints_offset_and_limit_params() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("offset_limit");
        let file_path = abs.join("test.txt");
        let rel_file = format!("{}/test.txt", rel);
        std::fs::write(&file_path, "line1\nline2\nline3")?;

        let app_state = app_state();
        let tool = ReadFileTool::new(&app_state);
        let result = tool
            .call(ReadFileArgs {
                path: rel_file.clone(),
                offset: Some(2),
                limit: Some(2),
            })
            .await?;

        assert!(
            result.starts_with("[read_file]"),
            "Output must start with '[read_file]', got: {}",
            result
        );
        assert!(
            result.contains("offset=2"),
            "Output must contain 'offset=2', got: {}",
            result
        );
        assert!(
            result.contains("limit=2"),
            "Output must contain 'limit=2', got: {}",
            result
        );
        cleanup_rel(&rel);
        Ok(())
    }

    #[tokio::test]
    async fn test_read_file_omits_known_lines_and_splits_unread_ranges() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("known_lines");
        let file_path = abs.join("test.txt");
        let rel_file = format!("{}/test.txt", rel);
        let content = (1..=130)
            .map(|line| format!("line{line}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&file_path, content)?;

        let app_state = app_state();
        let tool = ReadFileTool::new(&app_state);

        let first = tool
            .call(ReadFileArgs {
                path: rel_file.clone(),
                offset: Some(20),
                limit: Some(11),
            })
            .await?;
        assert!(first.ends_with(
            "line20\nline21\nline22\nline23\nline24\nline25\nline26\nline27\nline28\nline29\nline30"
        ));

        let known = tool
            .call(ReadFileArgs {
                path: rel_file.clone(),
                offset: Some(20),
                limit: Some(6),
            })
            .await
            .unwrap_err();
        assert_eq!(known.to_string(), "Context known");

        let preceding = tool
            .call(ReadFileArgs {
                path: rel_file.clone(),
                offset: Some(10),
                limit: Some(21),
            })
            .await?;
        assert!(preceding.ends_with(
            "line10\nline11\nline12\nline13\nline14\nline15\nline16\nline17\nline18\nline19"
        ));
        assert!(preceding.contains("[lines 10-19]"));
        assert!(!preceding.contains("line20"));

        let split = tool
            .call(ReadFileArgs {
                path: rel_file,
                offset: Some(1),
                limit: Some(100),
            })
            .await?;
        assert!(split.contains("[lines 1-9]\nline1"));
        assert!(split.contains("[lines 31-100]\nline31"));
        assert!(!split.contains("\nline20\n"));

        cleanup_rel(&rel);
        Ok(())
    }

    #[tokio::test]
    async fn test_read_file_forgets_ranges_when_file_changes() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("changed_file");
        let file_path = abs.join("test.txt");
        let rel_file = format!("{}/test.txt", rel);
        std::fs::write(&file_path, "before")?;

        let app_state = app_state();
        let tool = ReadFileTool::new(&app_state);
        let args = || ReadFileArgs {
            path: rel_file.clone(),
            offset: None,
            limit: None,
        };

        assert!(tool.call(args()).await?.ends_with("before"));
        assert_eq!(
            tool.call(args()).await.unwrap_err().to_string(),
            "Context known"
        );

        std::fs::write(&file_path, "after")?;
        assert!(tool.call(args()).await?.ends_with("after"));

        cleanup_rel(&rel);
        Ok(())
    }

    #[test]
    fn test_unread_ranges_handles_multiple_known_gaps() {
        let known = vec![(105, 110), (115, 120)];

        assert_eq!(
            unread_ranges((100, 130), &known),
            vec![(100, 105), (110, 115), (120, 130)]
        );
    }
}
