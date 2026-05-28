use std::path::Path;

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
pub struct WriteFileTool;

impl Tool for WriteFileTool {
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
                        "description": "Path of the new file to create"
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
        let path = Path::new(&args.path);

        if path.exists() {
            return Err(WriteFileError(format!(
                "File already exists: {}",
                args.path
            )));
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(path, &args.content)?;

        Ok(format!(
            "[write_file] path={} content={}\nSuccessfully created file: {}",
            args.path, args.content, args.path
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_write_file_new() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("subdir").join("new_file.txt");

        let tool = WriteFileTool;
        let result = tool
            .call(WriteFileArgs {
                path: file_path.display().to_string(),
                content: "hello world".into(),
            })
            .await?;

        assert!(result.contains("Successfully created"));
        assert!(file_path.exists());
        assert_eq!(std::fs::read_to_string(&file_path)?, "hello world");
        Ok(())
    }

    #[tokio::test]
    async fn test_write_file_rejects_existing() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("existing.txt");
        std::fs::write(&file_path, "old")?;

        let tool = WriteFileTool;
        let result = tool
            .call(WriteFileArgs {
                path: file_path.display().to_string(),
                content: "new".into(),
            })
            .await;

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&file_path)?, "old");
        Ok(())
    }

    #[tokio::test]
    async fn test_write_file_prints_tool_name_and_params() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("output.txt");
        let content = "hello world";

        let tool = WriteFileTool;
        let result = tool
            .call(WriteFileArgs {
                path: file_path.display().to_string(),
                content: content.into(),
            })
            .await?;

        assert!(
            result.starts_with("[write_file]"),
            "Output must start with '[write_file]', got: {}",
            result
        );
        assert!(
            result.contains(file_path.display().to_string().as_str()),
            "Output must contain the file path, got: {}",
            result
        );
        assert!(
            result.contains(content),
            "Output must contain the content, got: {}",
            result
        );
        Ok(())
    }
}
