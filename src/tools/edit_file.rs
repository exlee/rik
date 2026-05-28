use std::path::Path;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

// ---------------------------------------------------------------------------
// EditFile tool
// ---------------------------------------------------------------------------

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
#[derive(Deserialize, Serialize)]
pub struct EditFileTool;

impl Tool for EditFileTool {
    const NAME: &'static str = "edit_file";

    type Error = EditFileError;
    type Args = EditFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Edit an existing file by replacing exact text. \
                          old_text must match exactly one occurrence in the file. \
                          That text is replaced with new_text. \
                          Use this to replace marker lines with completed content."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path of the file to edit"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_edit_file_replace_line() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "before\nrik: write a poem\nafter\n")?;

        let tool = EditFileTool;
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
        let tool = EditFileTool;
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

        let tool = EditFileTool;
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

        let tool = EditFileTool;
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

        let tool = EditFileTool;
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
}
