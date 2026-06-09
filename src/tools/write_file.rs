use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

// ---------------------------------------------------------------------------
// WriteFile tool
// ---------------------------------------------------------------------------

/// Arguments for the write_file tool.
#[derive(Deserialize)]
#[allow(dead_code)]
pub struct WriteFileArgs {
    /// Path of the file to create.
    pub path: String,
    /// Full content to write into the file.
    pub content: String,
}

/// Error type for the write_file tool.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
#[allow(dead_code)]
pub struct WriteFileError(String);

impl From<std::io::Error> for WriteFileError {
    fn from(e: std::io::Error) -> Self {
        WriteFileError(e.to_string())
    }
}

/// A tool that writes content to a new (non-existing) file.
///
/// Returns an error if the file already exists.
#[derive(Deserialize, Serialize)]
#[allow(dead_code)]
pub struct WriteFileTool<'a> {
    #[serde(skip, default = "crate::state::get")]
    pub app_state: &'a crate::state::AppState,
}

impl Default for WriteFileTool<'static> {
    fn default() -> Self {
        Self {
            app_state: crate::state::get(),
        }
    }
}

impl Tool for WriteFileTool<'_> {
    const NAME: &'static str = "write_file";

    type Error = WriteFileError;
    type Args = WriteFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Write content to a new file. The file must not already exist. \
                          Returns an error if the path points to an existing file or directory."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path of the new file to create"
                    },
                    "content": {
                        "type": "string",
                        "description": "Full content to write into the file"
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = self
            .app_state
            .resolve_path(&args.path)
            .map_err(|e| WriteFileError(e.to_string()))?;
        if path.exists() {
            return Err(WriteFileError(format!(
                "File already exists: {}",
                path.display()
            )));
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&path, &args.content)?;

        Ok(format!(
            "[write_file] path={} content={}\nSuccessfully created file: {}",
            path.display(),
            args.content,
            path.display()
        ))
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
    async fn test_write_file_new() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("write_new");
        let rel_file = format!("{}/subdir/new_file.txt", rel);
        let abs_file = abs.join("subdir").join("new_file.txt");

        let app_state = app_state();
        let tool = WriteFileTool {
            app_state: &app_state,
        };
        let result = tool
            .call(WriteFileArgs {
                path: rel_file.clone(),
                content: "hello world".into(),
            })
            .await?;

        assert!(result.contains("Successfully created"));
        assert!(abs_file.exists());
        assert_eq!(std::fs::read_to_string(&abs_file)?, "hello world");
        cleanup_rel(&rel);
        Ok(())
    }

    #[tokio::test]
    async fn test_write_file_rejects_existing() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("write_exists");
        let rel_file = format!("{}/existing.txt", rel);
        let abs_file = abs.join("existing.txt");
        std::fs::write(&abs_file, "old")?;

        let app_state = app_state();
        let tool = WriteFileTool {
            app_state: &app_state,
        };
        let result = tool
            .call(WriteFileArgs {
                path: rel_file,
                content: "new".into(),
            })
            .await;

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&abs_file)?, "old");
        cleanup_rel(&rel);
        Ok(())
    }

    #[tokio::test]
    async fn test_write_file_rejects_absolute_path() {
        let app_state = app_state();
        let tool = WriteFileTool {
            app_state: &app_state,
        };
        let result = tool
            .call(WriteFileArgs {
                path: "/tmp/evil.txt".to_string(),
                content: "bad".into(),
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
    async fn test_write_file_rejects_path_traversal() {
        let app_state = app_state();
        let tool = WriteFileTool {
            app_state: &app_state,
        };
        let result = tool
            .call(WriteFileArgs {
                path: "../../etc/evil.txt".to_string(),
                content: "bad".into(),
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
    async fn test_write_file_prints_tool_name_and_params() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("write_params");
        let rel_file = format!("{}/output.txt", rel);
        let _ = abs.join("output.txt"); // suppress unused warning on abs
        let content = "hello world";

        let app_state = app_state();
        let tool = WriteFileTool {
            app_state: &app_state,
        };
        let result = tool
            .call(WriteFileArgs {
                path: rel_file.clone(),
                content: content.into(),
            })
            .await?;

        assert!(
            result.starts_with("[write_file]"),
            "Output must start with '[write_file]', got: {}",
            result
        );
        assert!(
            result.contains(&rel_file),
            "Output must contain the file path, got: {}",
            result
        );
        assert!(
            result.contains(content),
            "Output must contain the content, got: {}",
            result
        );
        cleanup_rel(&rel);
        Ok(())
    }
}
