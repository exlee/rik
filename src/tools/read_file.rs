use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt::Write;

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
#[derive(Deserialize, Serialize)]
pub struct ReadFileTool<'a> {
    #[serde(skip, default = "crate::state::get")]
    pub app_state: &'a crate::state::AppState,
}

impl Default for ReadFileTool<'static> {
    fn default() -> Self {
        Self {
            app_state: crate::state::get(),
        }
    }
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
                          to read only a portion of the file."
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

        let lines: Vec<&str> = content.lines().collect();

        let start = args.offset.unwrap_or(1).saturating_sub(1);
        let end = if let Some(limit) = args.limit {
            (start + limit).min(lines.len())
        } else {
            lines.len()
        };

        if start >= lines.len() {
            return Ok(String::new());
        }

        let result: Vec<&str> = lines[start..end].to_vec();
        let mut header = format!("[read_file] path={}", path.display());
        if let Some(offset) = args.offset {
            write!(header, " offset={offset}").ok();
        }
        if let Some(limit) = args.limit {
            write!(header, " limit={limit}").ok();
        }
        Ok(format!("{header}\n{}", result.join("\n")))
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
        let tool = ReadFileTool {
            app_state: &app_state,
        };
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
        let tool = ReadFileTool {
            app_state: &app_state,
        };
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
        let tool = ReadFileTool {
            app_state: &app_state,
        };
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
        let tool = ReadFileTool {
            app_state: &app_state,
        };
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
        let tool = ReadFileTool {
            app_state: &app_state,
        };
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
        let tool = ReadFileTool {
            app_state: &app_state,
        };
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
        let tool = ReadFileTool {
            app_state: &app_state,
        };
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
}
