use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;
use serde_json::json;

use crate::skills::{self, Skill};

// ---------------------------------------------------------------------------
// Skill tool
// ---------------------------------------------------------------------------

/// Arguments for the skill tool.
#[derive(Deserialize)]
pub struct SkillArgs {
    /// Name of the skill to load.
    pub name: String,
    /// Optional bundled file to read instead of the skill instructions.
    #[serde(default)]
    pub file: Option<String>,
}

/// Error type for the skill tool.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct SkillError(String);

/// A tool that loads user-maintained skills discovered under `~/.agents`,
/// `~/.codex`, and `~/.claude`.
pub struct SkillTool {
    skills: &'static [Skill],
}

impl Default for SkillTool {
    fn default() -> Self {
        Self {
            skills: skills::all(),
        }
    }
}

impl SkillTool {
    #[cfg(test)]
    fn new(skills: &'static [Skill]) -> Self {
        Self { skills }
    }

    fn unknown_skill(&self, name: &str) -> SkillError {
        let available = skills::invocable_names(self.skills);
        let available = if available.is_empty() {
            "none".to_string()
        } else {
            available.join(", ")
        };
        SkillError(format!(
            "Unknown skill '{name}'. Available skills: {available}"
        ))
    }
}

impl Tool for SkillTool {
    const NAME: &'static str = "skill";

    type Error = SkillError;
    type Args = SkillArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        let names = skills::invocable_names(self.skills).join(", ");
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: format!(
                "Load a named skill: user-maintained instructions and reference material \
                 for a kind of task. Call it before working when a skill matches the task, \
                 then follow what it says. Pass `file` to read one of the skill's bundled \
                 files. Available skills: {names}."
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the skill to load"
                    },
                    "file": {
                        "type": "string",
                        "description": "Optional path of a bundled skill file, relative to the skill directory"
                    }
                },
                "required": ["name"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let skill =
            skills::find(self.skills, &args.name).ok_or_else(|| self.unknown_skill(&args.name))?;
        let output = match args
            .file
            .as_deref()
            .map(str::trim)
            .filter(|f| !f.is_empty())
        {
            Some(file) => skill.read_resource(file),
            None => skill.instructions(),
        };
        output.map_err(|error| SkillError(format!("{error:#}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leak_skills(home: &std::path::Path) -> &'static [Skill] {
        Box::leak(skills::discover(home).into_boxed_slice())
    }

    fn write_skill(home: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        let directory = home.join(".agents/skills").join(name);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("SKILL.md"), body).unwrap();
        directory
    }

    #[tokio::test]
    async fn loads_skill_instructions_and_bundled_files() {
        let home = tempfile::tempdir().unwrap();
        let directory = write_skill(
            home.path(),
            "changelog-write",
            "---\nname: changelog-write\ndescription: Writes changelogs.\n---\n# Changelog\nSteps.\n",
        );
        std::fs::write(directory.join("template.md"), "## Unreleased").unwrap();
        let tool = SkillTool::new(leak_skills(home.path()));

        let instructions = tool
            .call(SkillArgs {
                name: "changelog-write".to_string(),
                file: None,
            })
            .await
            .unwrap();
        let file = tool
            .call(SkillArgs {
                name: "changelog-write".to_string(),
                file: Some("template.md".to_string()),
            })
            .await
            .unwrap();

        assert!(instructions.contains("# Changelog\nSteps."));
        assert!(instructions.contains("- template.md"));
        assert!(file.contains("## Unreleased"));
    }

    #[tokio::test]
    async fn reports_available_skills_for_an_unknown_name() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            "known",
            "---\nname: known\ndescription: Known.\n---\nBody.\n",
        );
        let tool = SkillTool::new(leak_skills(home.path()));

        let error = tool
            .call(SkillArgs {
                name: "missing".to_string(),
                file: None,
            })
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown skill 'missing'. Available skills: known"
        );
    }

    #[tokio::test]
    async fn refuses_bundled_files_outside_the_skill_directory() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            "refs",
            "---\nname: refs\ndescription: Refs.\n---\nBody.\n",
        );
        std::fs::write(home.path().join("secret.md"), "secret").unwrap();
        let tool = SkillTool::new(leak_skills(home.path()));

        let error = tool
            .call(SkillArgs {
                name: "refs".to_string(),
                file: Some("../../secret.md".to_string()),
            })
            .await
            .unwrap_err();

        assert!(error.to_string().contains("escapes the skill directory"));
    }
}
