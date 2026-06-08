use std::collections::HashSet;
use std::path::Path;

use rig::completion::ToolDefinition;
use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;
use serde_json::{Map, Value, json};

#[derive(Clone, Debug, PartialEq, Eq)]
enum CommandPart {
    Fixed(String),
    Parameter(String),
    Variadic,
}

#[derive(Clone, Debug)]
pub struct DynamicCommandTool {
    name: String,
    description: String,
    parts: Vec<CommandPart>,
    working_dir: std::path::PathBuf,
}

impl DynamicCommandTool {
    fn parse(line: &str, alias: &str, working_dir: &Path) -> Option<Self> {
        let start = line.find(&format!("{alias} +tool"))?;
        let rest = line[start + alias.len() + " +tool".len()..].trim_start();
        let (description, command) = if let Some(rest) = rest.strip_prefix('(') {
            let end = rest.find("):")?;
            (Some(rest[..end].trim().to_string()), rest[end + 2..].trim())
        } else {
            (None, rest.strip_prefix(':')?.trim())
        };
        let words = shlex::split(command)?;
        let executable = words.first()?.clone();
        let name = Path::new(&executable)
            .file_name()?
            .to_string_lossy()
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>();
        if name.is_empty() {
            return None;
        }

        let parts = words.into_iter().map(parse_part).collect();
        Some(Self {
            name,
            description: description.unwrap_or_else(|| format!("Run `{command}`.")),
            parts,
            working_dir: working_dir.to_path_buf(),
        })
    }

    fn parameters(&self) -> Value {
        let mut properties = Map::new();
        let mut required = Vec::new();
        for part in &self.parts {
            let (name, schema) = match part {
                CommandPart::Parameter(name) => (
                    name.clone(),
                    json!({
                        "type": "string",
                        "description": format!("Value substituted for <{}>", name.to_uppercase())
                    }),
                ),
                CommandPart::Variadic => (
                    "args".to_string(),
                    json!({
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Additional command arguments substituted for <...>"
                    }),
                ),
                CommandPart::Fixed(_) => continue,
            };
            if !properties.contains_key(&name) {
                properties.insert(name.clone(), schema);
                required.push(Value::String(name));
            }
        }
        json!({
            "type": "object",
            "properties": properties,
            "required": required
        })
    }

    fn command_args(&self, args: &Value) -> Result<Vec<String>, DynamicCommandError> {
        let object = args
            .as_object()
            .ok_or_else(|| DynamicCommandError("tool arguments must be an object".to_string()))?;
        let mut command = Vec::new();
        for part in &self.parts {
            match part {
                CommandPart::Fixed(value) => command.push(value.clone()),
                CommandPart::Parameter(name) => {
                    let value = object.get(name).and_then(Value::as_str).ok_or_else(|| {
                        DynamicCommandError(format!("missing string parameter: {name}"))
                    })?;
                    command.push(value.to_string());
                }
                CommandPart::Variadic => {
                    let values = object
                        .get("args")
                        .and_then(Value::as_array)
                        .ok_or_else(|| {
                            DynamicCommandError("missing string array parameter: args".to_string())
                        })?;
                    for value in values {
                        command.push(
                            value
                                .as_str()
                                .ok_or_else(|| {
                                    DynamicCommandError(
                                        "all args values must be strings".to_string(),
                                    )
                                })?
                                .to_string(),
                        );
                    }
                }
            }
        }
        Ok(command)
    }
}

fn parse_part(word: String) -> CommandPart {
    if word == "<...>" {
        return CommandPart::Variadic;
    }
    if word.starts_with('<') && word.ends_with('>') && word.len() > 2 {
        return CommandPart::Parameter(word[1..word.len() - 1].to_ascii_lowercase());
    }
    CommandPart::Fixed(word)
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct DynamicCommandError(String);

impl ToolDyn for DynamicCommandTool {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn definition<'a>(&'a self, _prompt: String) -> WasmBoxedFuture<'a, ToolDefinition> {
        Box::pin(async move {
            ToolDefinition {
                name: self.name.clone(),
                description: self.description.clone(),
                parameters: self.parameters(),
            }
        })
    }

    fn call<'a>(&'a self, args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        Box::pin(async move {
            let args: Value = serde_json::from_str(&args).map_err(ToolError::JsonError)?;
            let command = self
                .command_args(&args)
                .map_err(|error| ToolError::ToolCallError(Box::new(error)))?;
            let (executable, arguments) = command.split_first().ok_or_else(|| {
                ToolError::ToolCallError(Box::new(DynamicCommandError(
                    "command is empty".to_string(),
                )))
            })?;
            let output = tokio::process::Command::new(executable)
                .args(arguments)
                .current_dir(&self.working_dir)
                .output()
                .await
                .map_err(|error| ToolError::ToolCallError(Box::new(error)))?;

            let mut result = String::new();
            if !output.stdout.is_empty() {
                result.push_str(&String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                if !result.is_empty() && !result.ends_with('\n') {
                    result.push('\n');
                }
                result.push_str(&String::from_utf8_lossy(&output.stderr));
            }
            if !output.status.success() {
                if !result.is_empty() && !result.ends_with('\n') {
                    result.push('\n');
                }
                result.push_str(&format!("[exit status: {}]", output.status));
            }
            Ok(result)
        })
    }
}

pub fn find_dynamic_tools(content: &str, alias: &str, working_dir: &Path) -> Vec<Box<dyn ToolDyn>> {
    let mut names = HashSet::new();
    content
        .lines()
        .filter_map(|line| DynamicCommandTool::parse(line, alias, working_dir))
        .filter(|tool| names.insert(tool.name.clone()))
        .map(|tool| Box::new(tool) as Box<dyn ToolDyn>)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixed_named_and_variadic_arguments() {
        let tool = DynamicCommandTool::parse(
            "rik +tool (read files): cat <PATH> <...>",
            "rik",
            Path::new("/tmp"),
        )
        .unwrap();
        assert_eq!(tool.name, "cat");
        assert_eq!(tool.description, "read files");
        assert_eq!(
            tool.command_args(&json!({"path": "a.txt", "args": ["b.txt", "-n"]}))
                .unwrap(),
            ["cat", "a.txt", "b.txt", "-n"]
        );
        assert_eq!(tool.parameters()["required"], json!(["path", "args"]));
    }

    #[test]
    fn finds_only_matching_alias_and_deduplicates_names() {
        let tools = find_dynamic_tools(
            "rik +tool: cargo test\nother +tool: zig test x\nrik +tool: cargo check",
            "rik",
            Path::new("/tmp"),
        );
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "cargo");
    }

    #[tokio::test]
    async fn executes_fixed_command_without_a_shell() {
        let tool =
            DynamicCommandTool::parse("rik +tool: rustc --version", "rik", Path::new("/tmp"))
                .unwrap();
        let output = ToolDyn::call(&tool, "{}".to_string()).await.unwrap();

        assert!(output.starts_with("rustc "));
    }
}
