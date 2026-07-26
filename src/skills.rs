//! Agent skill discovery.
//!
//! Skills are directories containing a `SKILL.md` file with YAML frontmatter
//! (`name`, `description`, optionally `allowed-tools` and
//! `disable-model-invocation`) plus any number of bundled resource files. Rik
//! looks for `skills/` subdirectories under `~/.agents`, `~/.codex`, and
//! `~/.claude`, in that order; when the same skill name exists in more than one
//! location the earliest one wins.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};

/// Maximum number of bundled files listed for a single skill.
const MAX_LISTED_RESOURCES: usize = 40;
/// Maximum size of a bundled resource handed back to the model.
const MAX_RESOURCE_BYTES: usize = 200 * 1024;
/// How deep resource discovery walks into a skill directory.
const MAX_RESOURCE_DEPTH: usize = 3;

/// Home directory that provided a skill. Ordering is the precedence order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillSource {
    Agents,
    Codex,
    Claude,
}

impl SkillSource {
    /// Sources in precedence order: `.agents` beats `.codex` beats `.claude`.
    pub const ALL: [Self; 3] = [Self::Agents, Self::Codex, Self::Claude];

    /// Directory below the home directory holding the skills of this source.
    fn relative_dir(self) -> &'static str {
        match self {
            Self::Agents => ".agents/skills",
            Self::Codex => ".codex/skills",
            Self::Claude => ".claude/skills",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Agents => "~/.agents",
            Self::Codex => "~/.codex",
            Self::Claude => "~/.claude",
        }
    }
}

/// A discovered skill.
#[derive(Clone, Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Tools the skill declares it needs. Informational only — Rik always
    /// exposes its own tool set.
    pub allowed_tools: Vec<String>,
    /// False when the skill sets `disable-model-invocation: true`; such skills
    /// stay out of the catalog but can still be loaded by exact name.
    pub model_invocable: bool,
    pub source: SkillSource,
    pub directory: PathBuf,
}

impl Skill {
    fn skill_file(&self) -> PathBuf {
        self.directory.join("SKILL.md")
    }

    /// The skill body (frontmatter stripped) plus a list of bundled files.
    pub fn instructions(&self) -> Result<String> {
        let path = self.skill_file();
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read skill: {}", path.display()))?;
        let body = strip_frontmatter(&contents).trim();

        let mut output = format!(
            "[skill] name={} source={}\n",
            self.name,
            self.source.label()
        );
        if !self.allowed_tools.is_empty() {
            output.push_str(&format!(
                "Tools this skill expects: {}. Rik only offers its own tools; \
                 skip steps you cannot perform.\n",
                self.allowed_tools.join(", ")
            ));
        }
        output.push('\n');
        output.push_str(body);

        let resources = self.resources();
        if !resources.is_empty() {
            output.push_str(&format!(
                "\n\nBundled files (load with skill(name=\"{}\", file=\"<path>\")):\n",
                self.name
            ));
            for resource in &resources {
                output.push_str(&format!("- {resource}\n"));
            }
        }
        Ok(output)
    }

    /// Read a file bundled with the skill. The path must stay inside the
    /// skill directory.
    pub fn read_resource(&self, relative: &str) -> Result<String> {
        let path = self.resolve_resource(relative)?;
        let metadata = std::fs::metadata(&path)
            .with_context(|| format!("Failed to read skill file: {relative}"))?;
        if metadata.len() as usize > MAX_RESOURCE_BYTES {
            anyhow::bail!(
                "Skill file is too large ({} bytes, limit {MAX_RESOURCE_BYTES}): {relative}",
                metadata.len()
            );
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read skill file: {relative}"))?;
        Ok(format!(
            "[skill] name={} file={relative}\n\n{contents}",
            self.name
        ))
    }

    fn resolve_resource(&self, relative: &str) -> Result<PathBuf> {
        let requested = Path::new(relative);
        if requested.is_absolute() {
            anyhow::bail!("Skill files must be referenced by relative path: {relative}");
        }
        let mut resolved = self.directory.clone();
        for component in requested.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(part) => resolved.push(part),
                _ => anyhow::bail!("Skill file path escapes the skill directory: {relative}"),
            }
        }
        if !resolved.starts_with(&self.directory) {
            anyhow::bail!("Skill file path escapes the skill directory: {relative}");
        }
        Ok(resolved)
    }

    /// Relative paths of the files bundled next to `SKILL.md`.
    pub fn resources(&self) -> Vec<String> {
        let mut resources = Vec::new();
        collect_resources(&self.directory, &self.directory, 0, &mut resources);
        resources.sort();
        resources.truncate(MAX_LISTED_RESOURCES);
        resources
    }
}

fn collect_resources(root: &Path, directory: &Path, depth: usize, resources: &mut Vec<String>) {
    if depth > MAX_RESOURCE_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_resources(root, &path, depth + 1, resources);
            continue;
        }
        if name == "SKILL.md" {
            continue;
        }
        if let Ok(relative) = path.strip_prefix(root) {
            resources.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Discover skills under the user's home directory.
pub fn all() -> &'static [Skill] {
    static SKILLS: OnceLock<Vec<Skill>> = OnceLock::new();
    SKILLS.get_or_init(|| match dirs::home_dir() {
        Some(home) => discover(&home),
        None => Vec::new(),
    })
}

/// Discover skills below `home`, honoring source precedence.
pub fn discover(home: &Path) -> Vec<Skill> {
    let mut skills: Vec<Skill> = Vec::new();
    for source in SkillSource::ALL {
        for skill in discover_in(&home.join(source.relative_dir()), source) {
            if skills.iter().any(|known| known.name == skill.name) {
                continue;
            }
            skills.push(skill);
        }
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    skills
}

fn discover_in(root: &Path, source: SkillSource) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut skills: Vec<Skill> = entries
        .flatten()
        .filter_map(|entry| load_skill(&entry.path(), source))
        .collect();
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    skills
}

fn load_skill(directory: &Path, source: SkillSource) -> Option<Skill> {
    if !directory.is_dir() {
        return None;
    }
    let contents = std::fs::read_to_string(directory.join("SKILL.md")).ok()?;
    let fields = parse_frontmatter(&contents);
    let directory_name = directory.file_name()?.to_string_lossy().to_string();
    let name = fields
        .get("name")
        .filter(|name| !name.is_empty())
        .cloned()
        .unwrap_or(directory_name);
    let description = fields.get("description").cloned().unwrap_or_default();
    if description.is_empty() {
        return None;
    }
    let allowed_tools = fields
        .get("allowed-tools")
        .map(|tools| {
            tools
                .split(',')
                .map(|tool| tool.trim().trim_matches(['[', ']', '"', '\'']).to_string())
                .filter(|tool| !tool.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let model_invocable = !fields
        .get("disable-model-invocation")
        .is_some_and(|value| matches!(value.as_str(), "true" | "yes"));

    Some(Skill {
        name,
        description,
        allowed_tools,
        model_invocable,
        source,
        directory: directory.to_path_buf(),
    })
}

/// Look a skill up by name, case-insensitively.
pub fn find<'a>(skills: &'a [Skill], name: &str) -> Option<&'a Skill> {
    let name = name.trim();
    skills.iter().find(|skill| skill.name == name).or_else(|| {
        skills
            .iter()
            .find(|skill| skill.name.eq_ignore_ascii_case(name))
    })
}

/// Names of the skills a model may pick on its own.
pub fn invocable_names(skills: &[Skill]) -> Vec<&str> {
    skills
        .iter()
        .filter(|skill| skill.model_invocable)
        .map(|skill| skill.name.as_str())
        .collect()
}

/// Resolve skill names requested on the command line. Unknown names fail with
/// the full list of available skills. Explicitly requested skills may include
/// ones the model is not allowed to pick on its own.
pub fn resolve_requested<'a>(skills: &'a [Skill], requested: &[String]) -> Result<Vec<&'a Skill>> {
    let mut resolved: Vec<&Skill> = Vec::new();
    for name in requested {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let skill = find(skills, name).ok_or_else(|| {
            anyhow::anyhow!("Unknown skill '{name}'. {}", available_listing(skills))
        })?;
        if !resolved.iter().any(|known| known.name == skill.name) {
            resolved.push(skill);
        }
    }
    Ok(resolved)
}

/// Human-facing list of every discovered skill.
pub fn available_listing(skills: &[Skill]) -> String {
    if skills.is_empty() {
        let searched = SkillSource::ALL
            .iter()
            .map(|source| format!("{}/skills", source.label()))
            .collect::<Vec<_>>()
            .join(", ");
        return format!("No skills found. Searched: {searched}.");
    }

    let entries = skills
        .iter()
        .map(|skill| {
            format!(
                "  {} ({}){}: {}",
                skill.name,
                skill.source.label(),
                if skill.model_invocable {
                    ""
                } else {
                    " [manual]"
                },
                skill.description
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("Available skills:\n{entries}")
}

/// Preamble section for a run: the full instructions of every preloaded skill,
/// followed by a catalog of the remaining skills the model may load itself.
pub fn prompt_section(skills: &[Skill], preloaded: &[&Skill]) -> Result<String> {
    let mut section = String::new();

    if !preloaded.is_empty() {
        section.push_str(
            "\n\nPreloaded skills — the user selected these for this run. Follow them \
             where they apply to the task; they are already loaded, so do not fetch \
             them again with the skill tool.\n",
        );
        for skill in preloaded {
            section.push('\n');
            section.push_str(&skill.instructions()?);
            section.push('\n');
        }
    }

    let catalog: Vec<_> = skills
        .iter()
        .filter(|skill| {
            skill.model_invocable && !preloaded.iter().any(|loaded| loaded.name == skill.name)
        })
        .collect();
    if !catalog.is_empty() {
        section.push_str(&catalog_section(&catalog));
    }

    Ok(section)
}

fn catalog_section(catalog: &[&Skill]) -> String {
    let entries = catalog
        .iter()
        .map(|skill| format!("- {}: {}", skill.name, skill.description))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "\n\nSkills — user-maintained instructions and reference material:\n{entries}\n\
         Call skill(name=\"<name>\") to load one BEFORE doing the work whenever its \
         description matches the task, and follow the loaded instructions. Fetch a \
         bundled file with skill(name=\"<name>\", file=\"<relative path>\"). Do not \
         guess at a skill's contents, and do not load skills unrelated to the task."
    )
}

/// Everything before and including the closing `---` of a leading YAML
/// frontmatter block is removed. Content without frontmatter is returned as-is.
fn strip_frontmatter(contents: &str) -> &str {
    let rest = match contents.strip_prefix("---\n") {
        Some(rest) => rest,
        None => match contents.strip_prefix("---\r\n") {
            Some(rest) => rest,
            None => return contents,
        },
    };
    let mut cursor = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        cursor += line.len();
        if trimmed == "---" || trimmed == "..." {
            return &rest[cursor..];
        }
    }
    contents
}

/// Minimal YAML frontmatter reader: `key: value` pairs with support for
/// indented continuation lines and `>`/`|` block scalars.
fn parse_frontmatter(contents: &str) -> std::collections::HashMap<String, String> {
    let mut fields = std::collections::HashMap::new();
    let Some(rest) = contents
        .strip_prefix("---\n")
        .or_else(|| contents.strip_prefix("---\r\n"))
    else {
        return fields;
    };

    let mut current: Option<(String, String)> = None;
    for line in rest.lines() {
        let trimmed_end = line.trim_end();
        if trimmed_end == "---" || trimmed_end == "..." {
            break;
        }
        if trimmed_end.is_empty() {
            continue;
        }

        let is_continuation = line.starts_with([' ', '\t']);
        let key_value = (!is_continuation)
            .then(|| trimmed_end.split_once(':'))
            .flatten();

        match key_value {
            Some((key, value)) => {
                if let Some((key, value)) = current.take() {
                    fields.insert(key, value);
                }
                let key = key.trim().to_ascii_lowercase();
                current = Some((key, clean_value(value)));
            }
            None => {
                if let Some((_, value)) = current.as_mut() {
                    let addition = trimmed_end.trim();
                    if !value.is_empty() {
                        value.push(' ');
                    }
                    value.push_str(addition);
                }
            }
        }
    }
    if let Some((key, value)) = current {
        fields.insert(key, value);
    }
    fields
}

fn clean_value(value: &str) -> String {
    let value = value.trim();
    let value = value
        .strip_prefix('>')
        .or_else(|| value.strip_prefix('|'))
        .unwrap_or(value);
    let value = value.trim_start_matches(['-', '+']).trim();
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(root: &Path, source: &str, name: &str, body: &str) -> PathBuf {
        let directory = root.join(source).join("skills").join(name);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("SKILL.md"), body).unwrap();
        directory
    }

    #[test]
    fn discovers_skills_from_every_supported_home_directory() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            ".agents",
            "alpha",
            "---\nname: alpha\ndescription: From agents.\n---\nAlpha body.\n",
        );
        write_skill(
            home.path(),
            ".codex",
            "beta",
            "---\nname: beta\ndescription: From codex.\n---\nBeta body.\n",
        );
        write_skill(
            home.path(),
            ".claude",
            "gamma",
            "---\nname: gamma\ndescription: From claude.\n---\nGamma body.\n",
        );

        let skills = discover(home.path());

        assert_eq!(
            skills
                .iter()
                .map(|skill| (skill.name.as_str(), skill.source))
                .collect::<Vec<_>>(),
            vec![
                ("alpha", SkillSource::Agents),
                ("beta", SkillSource::Codex),
                ("gamma", SkillSource::Claude),
            ]
        );
    }

    #[test]
    fn agents_wins_over_codex_and_codex_wins_over_claude() {
        let home = tempfile::tempdir().unwrap();
        for source in [".claude", ".codex", ".agents"] {
            write_skill(
                home.path(),
                source,
                "shared",
                &format!("---\nname: shared\ndescription: From {source}.\n---\nBody.\n"),
            );
        }
        write_skill(
            home.path(),
            ".claude",
            "codex-vs-claude",
            "---\nname: codex-vs-claude\ndescription: From claude.\n---\nBody.\n",
        );
        write_skill(
            home.path(),
            ".codex",
            "codex-vs-claude",
            "---\nname: codex-vs-claude\ndescription: From codex.\n---\nBody.\n",
        );

        let skills = discover(home.path());

        let shared = find(&skills, "shared").unwrap();
        assert_eq!(shared.source, SkillSource::Agents);
        assert_eq!(shared.description, "From .agents.");
        assert_eq!(
            find(&skills, "codex-vs-claude").unwrap().source,
            SkillSource::Codex
        );
    }

    #[test]
    fn skips_directories_without_a_usable_skill_file() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".agents/skills/empty")).unwrap();
        write_skill(
            home.path(),
            ".agents",
            "no-description",
            "---\nname: no-description\n---\nBody.\n",
        );
        write_skill(
            home.path(),
            ".agents",
            "ok",
            "---\ndescription: Fine.\n---\nBody.\n",
        );

        let skills = discover(home.path());

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "ok");
    }

    #[test]
    fn reads_frontmatter_fields_including_wrapped_descriptions() {
        let fields = parse_frontmatter(
            "---\nname: wrapped\ndescription: First part\n  second part\nallowed-tools: bash, read\ndisable-model-invocation: true\n---\nBody\n",
        );

        assert_eq!(fields["name"], "wrapped");
        assert_eq!(fields["description"], "First part second part");
        assert_eq!(fields["allowed-tools"], "bash, read");
        assert_eq!(fields["disable-model-invocation"], "true");
    }

    #[test]
    fn strips_frontmatter_from_the_body_only_when_present() {
        assert_eq!(
            strip_frontmatter("---\nname: x\n---\n# Title\nBody\n"),
            "# Title\nBody\n"
        );
        assert_eq!(strip_frontmatter("# Title\nBody\n"), "# Title\nBody\n");
        assert_eq!(
            strip_frontmatter("---\nunterminated\n"),
            "---\nunterminated\n"
        );
    }

    #[test]
    fn instructions_include_body_expected_tools_and_bundled_files() {
        let home = tempfile::tempdir().unwrap();
        let directory = write_skill(
            home.path(),
            ".agents",
            "clips",
            "---\nname: clips\ndescription: CLIPS help.\nallowed-tools: read, bash\n---\n# CLIPS\nUse the reference.\n",
        );
        std::fs::write(directory.join("reference.md"), "reference").unwrap();
        std::fs::write(directory.join(".DS_Store"), "junk").unwrap();
        std::fs::create_dir_all(directory.join("nested")).unwrap();
        std::fs::write(directory.join("nested/extra.md"), "extra").unwrap();

        let skills = discover(home.path());
        let instructions = find(&skills, "clips").unwrap().instructions().unwrap();

        assert!(instructions.starts_with("[skill] name=clips source=~/.agents\n"));
        assert!(instructions.contains("Tools this skill expects: read, bash"));
        assert!(instructions.contains("# CLIPS\nUse the reference."));
        assert!(instructions.contains("- reference.md"));
        assert!(instructions.contains("- nested/extra.md"));
        assert!(!instructions.contains("DS_Store"));
    }

    #[test]
    fn resources_are_read_only_from_inside_the_skill_directory() {
        let home = tempfile::tempdir().unwrap();
        let directory = write_skill(
            home.path(),
            ".agents",
            "refs",
            "---\nname: refs\ndescription: Refs.\n---\nBody.\n",
        );
        std::fs::write(directory.join("guide.md"), "guide body").unwrap();
        std::fs::write(home.path().join("secret.md"), "secret").unwrap();

        let skills = discover(home.path());
        let skill = find(&skills, "refs").unwrap();

        assert!(
            skill
                .read_resource("guide.md")
                .unwrap()
                .contains("guide body")
        );
        assert!(skill.read_resource("../../../secret.md").is_err());
        assert!(
            skill
                .read_resource(home.path().join("secret.md").to_string_lossy().as_ref())
                .is_err()
        );
    }

    #[test]
    fn model_invocation_can_be_disabled_without_hiding_the_skill_from_lookup() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            ".agents",
            "manual",
            "---\nname: manual\ndescription: Manual only.\ndisable-model-invocation: true\n---\nBody.\n",
        );
        write_skill(
            home.path(),
            ".agents",
            "auto",
            "---\nname: auto\ndescription: Automatic.\n---\nBody.\n",
        );

        let skills = discover(home.path());
        let section = prompt_section(&skills, &[]).unwrap();

        assert_eq!(invocable_names(&skills), vec!["auto"]);
        assert!(section.contains("- auto: Automatic."));
        assert!(!section.contains("manual"));
        assert!(find(&skills, "MANUAL").is_some());
    }

    #[test]
    fn prompt_section_is_empty_without_invocable_skills() {
        assert!(prompt_section(&[], &[]).unwrap().is_empty());
    }

    #[test]
    fn preloaded_skills_are_inlined_and_left_out_of_the_catalog() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            ".agents",
            "preloaded",
            "---\nname: preloaded\ndescription: Preloaded one.\n---\n# Preloaded\nAlways do this.\n",
        );
        write_skill(
            home.path(),
            ".agents",
            "other",
            "---\nname: other\ndescription: Other one.\n---\nBody.\n",
        );

        let skills = discover(home.path());
        let preloaded = resolve_requested(&skills, &["preloaded".to_string()]).unwrap();
        let section = prompt_section(&skills, &preloaded).unwrap();

        assert!(section.contains("Preloaded skills"));
        assert!(section.contains("# Preloaded\nAlways do this."));
        assert!(section.contains("- other: Other one."));
        assert!(!section.contains("- preloaded: Preloaded one."));
    }

    #[test]
    fn requested_skills_are_deduplicated_and_may_be_manual_only() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            ".agents",
            "manual",
            "---\nname: manual\ndescription: Manual only.\ndisable-model-invocation: true\n---\nManual body.\n",
        );

        let skills = discover(home.path());
        let preloaded = resolve_requested(
            &skills,
            &["MANUAL".to_string(), "manual".to_string(), String::new()],
        )
        .unwrap();

        assert_eq!(preloaded.len(), 1);
        assert!(
            prompt_section(&skills, &preloaded)
                .unwrap()
                .contains("Manual body.")
        );
    }

    #[test]
    fn unknown_requested_skill_lists_every_available_skill() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            home.path(),
            ".agents",
            "known",
            "---\nname: known\ndescription: Known one.\n---\nBody.\n",
        );
        write_skill(
            home.path(),
            ".claude",
            "hidden",
            "---\nname: hidden\ndescription: Hidden one.\ndisable-model-invocation: true\n---\nBody.\n",
        );

        let skills = discover(home.path());
        let error = resolve_requested(&skills, &["nope".to_string()])
            .unwrap_err()
            .to_string();

        assert!(error.starts_with("Unknown skill 'nope'. Available skills:"));
        assert!(error.contains("known (~/.agents): Known one."));
        assert!(error.contains("hidden (~/.claude) [manual]: Hidden one."));
    }

    #[test]
    fn unknown_requested_skill_reports_searched_directories_when_none_exist() {
        let home = tempfile::tempdir().unwrap();

        let error = resolve_requested(&discover(home.path()), &["nope".to_string()])
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            "Unknown skill 'nope'. No skills found. Searched: ~/.agents/skills, \
             ~/.codex/skills, ~/.claude/skills."
        );
    }
}
