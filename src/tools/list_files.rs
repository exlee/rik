use std::fmt::Write;
use std::path::Path;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

// ---------------------------------------------------------------------------
// ListFiles tool
// ---------------------------------------------------------------------------

/// Arguments for the list_files tool.
#[derive(Deserialize)]
pub struct ListFilesArgs {
    /// Directory to list files from. Defaults to current working directory.
    pub path: Option<String>,
    /// Optional glob pattern to filter results (e.g. "**/*.rs").
    pub glob: Option<String>,
}

/// Error type for the list_files tool.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ListFilesError(String);

/// A tool that lists files in a directory, respecting .gitignore and .ignore.
/// Returns absolute paths.
#[derive(Deserialize, Serialize)]
pub struct ListFilesTool;

impl Tool for ListFilesTool {
    const NAME: &'static str = "list_files";

    type Error = ListFilesError;
    type Args = ListFilesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List files in a directory. Respects .gitignore and .ignore rules. \
                          Returns absolute paths. Optionally filter by glob pattern."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory to list from (default: current working directory)"
                    },
                    "glob": {
                        "type": "string",
                        "description": "Optional glob to filter results (e.g. \"**/*.rs\", \"*.toml\")"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Validate the requested directory stays within cwd.
        let dir_arg = args.path.as_deref().unwrap_or(".");
        let _validated = crate::helpers::validate_relative_path(dir_arg)
            .map_err(|e| ListFilesError(e.to_string()))?;

        let root = Path::new(dir_arg);

        let root = if root.is_relative() {
            std::env::current_dir()
                .map_err(|e| ListFilesError(e.to_string()))?
                .join(root)
        } else {
            return Err(ListFilesError(
                "Absolute paths are not allowed".to_string(),
            ));
        };

        if !root.exists() {
            return Err(ListFilesError(format!(
                "Directory not found: {dir_arg}"
            )));
        }

        let mut builder = ignore::WalkBuilder::new(&root);
        builder
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .ignore(true)
            .parents(true);

        if let Some(ref glob_pattern) = args.glob {
            let glob = ignore::overrides::OverrideBuilder::new(&root)
                .add(glob_pattern)
                .map_err(|e| ListFilesError(format!("Invalid glob: {e}")))?
                .build()
                .map_err(|e| ListFilesError(format!("Invalid glob: {e}")))?;
            builder.overrides(glob);
        }

        let cwd = std::env::current_dir()
            .map_err(|e| ListFilesError(e.to_string()))?;
        let mut paths: Vec<String> = Vec::new();
        for entry in builder.build().filter_map(|e| e.ok()) {
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                if let Ok(rel) = entry.path().strip_prefix(&cwd) {
                    paths.push(rel.display().to_string());
                } else {
                    paths.push(entry.path().display().to_string());
                }
            }
        }

        if paths.is_empty() {
            return Ok("No files found.".to_string());
        }

        let mut header = format!("[list_files] path={dir_arg}");
        if let Some(ref glob_pattern) = args.glob {
            write!(header, " glob={glob_pattern}").ok();
        }
        Ok(format!("{header}\n{}", paths.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    async fn test_list_files_basic() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("list_basic");
        std::fs::write(abs.join("a.txt"), "a")?;
        std::fs::write(abs.join("b.rs"), "b")?;
        std::fs::create_dir(abs.join("sub"))?;
        std::fs::write(abs.join("sub").join("c.txt"), "c")?;

        let tool = ListFilesTool;
        let result = tool
            .call(ListFilesArgs {
                path: Some(rel.clone()),
                glob: None,
            })
            .await?;

        let files: Vec<&str> = result.lines().skip(1).collect();
        assert_eq!(files.len(), 3);
        assert!(result.contains("a.txt"));
        assert!(result.contains("b.rs"));
        assert!(result.contains("sub"));
        assert!(result.contains("c.txt"));
        cleanup_rel(&rel);
        Ok(())
    }

    #[tokio::test]
    async fn test_list_files_respects_gitignore() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("list_ignore");
        std::fs::write(abs.join("a.txt"), "a")?;
        std::fs::write(abs.join("ignored.log"), "log")?;
        std::fs::write(abs.join(".ignore"), "*.log\n")?;

        let tool = ListFilesTool;
        let result = tool
            .call(ListFilesArgs {
                path: Some(rel.clone()),
                glob: None,
            })
            .await?;

        assert!(result.contains("a.txt"));
        assert!(!result.contains("ignored.log"));
        assert!(result.contains(".ignore"));
        cleanup_rel(&rel);
        Ok(())
    }

    #[tokio::test]
    async fn test_list_files_with_glob() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("list_glob");
        std::fs::write(abs.join("a.txt"), "a")?;
        std::fs::write(abs.join("b.rs"), "b")?;
        std::fs::write(abs.join("c.rs"), "c")?;

        let tool = ListFilesTool;
        let result = tool
            .call(ListFilesArgs {
                path: Some(rel.clone()),
                glob: Some("*.rs".to_string()),
            })
            .await?;

        assert!(!result.contains("a.txt"));
        assert!(result.contains("b.rs"));
        assert!(result.contains("c.rs"));
        cleanup_rel(&rel);
        Ok(())
    }

    #[tokio::test]
    async fn test_list_files_not_found() {
        let tool = ListFilesTool;
        let result = tool
            .call(ListFilesArgs {
                path: Some(".rik_test_nonexistent/dir".to_string()),
                glob: None,
            })
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_list_files_rejects_absolute_path() {
        let tool = ListFilesTool;
        let result = tool
            .call(ListFilesArgs {
                path: Some("/tmp".to_string()),
                glob: None,
            })
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Absolute paths are not allowed"));
    }

    #[tokio::test]
    async fn test_list_files_rejects_path_traversal() {
        let tool = ListFilesTool;
        let result = tool
            .call(ListFilesArgs {
                path: Some("../../tmp".to_string()),
                glob: None,
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("escapes current directory"),
            "Expected path traversal rejection, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_list_files_prints_tool_name_and_params() -> anyhow::Result<()> {
        let (_abs, rel) = make_relative_dir("list_params");
        // We need to write a file inside the dir for it to show up.
        // Since we don't have abs here, recreate it:
        let abs = std::env::current_dir().unwrap().join(&rel);
        std::fs::write(abs.join("a.txt"), "a")?;

        let tool = ListFilesTool;
        let result = tool
            .call(ListFilesArgs {
                path: Some(rel.clone()),
                glob: None,
            })
            .await?;

        assert!(
            result.starts_with("[list_files]"),
            "Output must start with '[list_files]', got: {}",
            result
        );
        assert!(
            result.contains(&rel),
            "Output must contain the directory path, got: {}",
            result
        );
        cleanup_rel(&rel);
        Ok(())
    }

    #[tokio::test]
    async fn test_list_files_prints_glob_param() -> anyhow::Result<()> {
        let (abs, rel) = make_relative_dir("list_glob_param");
        std::fs::write(abs.join("a.txt"), "a")?;
        std::fs::write(abs.join("b.rs"), "b")?;

        let tool = ListFilesTool;
        let result = tool
            .call(ListFilesArgs {
                path: Some(rel.clone()),
                glob: Some("*.rs".to_string()),
            })
            .await?;

        assert!(
            result.starts_with("[list_files]"),
            "Output must start with '[list_files]', got: {}",
            result
        );
        assert!(
            result.contains("glob=*.rs"),
            "Output must contain 'glob=*.rs', got: {}",
            result
        );
        cleanup_rel(&rel);
        Ok(())
    }
}
