use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};

use crate::config::Config;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct QuestionId {
    file_path: PathBuf,
    start_line: usize,
    end_line: usize,
    query: String,
}

#[derive(Debug)]
pub struct AppState {
    pub path: PathBuf,
    pub config: Config,
    answered_questions: dashmap::DashSet<QuestionId>,
}

impl AppState {
    pub fn new(path: PathBuf, config: Config) -> Result<Self> {
        let path = path
            .canonicalize()
            .with_context(|| format!("Failed to resolve watched directory: {}", path.display()))?;
        if !path.is_dir() {
            anyhow::bail!("Watched path is not a directory: {}", path.display());
        }
        Ok(Self {
            path,
            config,
            answered_questions: dashmap::DashSet::new(),
        })
    }

    pub fn question_was_answered(
        &self,
        file_path: &Path,
        marker: &crate::markers::FoundMarker,
    ) -> bool {
        self.answered_questions
            .contains(&QuestionId::new(file_path, marker))
    }

    pub fn remember_answered_question(
        &self,
        file_path: &Path,
        marker: &crate::markers::FoundMarker,
    ) {
        self.answered_questions
            .insert(QuestionId::new(file_path, marker));
    }

    pub fn resolve_path(&self, raw: &str) -> Result<PathBuf> {
        let input = Path::new(raw);
        let joined = if input.is_absolute() {
            input.to_path_buf()
        } else {
            self.path.join(input)
        };
        let normalized = normalize(&joined)?;
        let mut existing = normalized.as_path();
        let mut suffix = Vec::new();
        while !existing.exists() {
            suffix.push(
                existing
                    .file_name()
                    .ok_or_else(|| anyhow::anyhow!("Could not resolve path: {raw}"))?,
            );
            existing = existing
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Could not resolve path: {raw}"))?;
        }
        let resolved_existing = existing
            .canonicalize()
            .with_context(|| format!("Failed to resolve path: {}", existing.display()))?;
        if !resolved_existing.starts_with(&self.path) {
            anyhow::bail!(
                "Path is outside watched directory {}: {}",
                self.path.display(),
                raw
            );
        }

        let mut resolved = resolved_existing;
        for component in suffix.into_iter().rev() {
            resolved.push(component);
        }
        Ok(resolved)
    }
}

impl QuestionId {
    fn new(file_path: &Path, marker: &crate::markers::FoundMarker) -> Self {
        Self {
            file_path: file_path.to_path_buf(),
            start_line: marker.start_line,
            end_line: marker.end_line,
            query: marker.query.clone(),
        }
    }
}

fn normalize(path: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    anyhow::bail!("Path escapes watched directory: {}", path.display());
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

static APP_STATE: OnceLock<AppState> = OnceLock::new();

pub fn init(path: PathBuf, config: Config) -> Result<&'static AppState> {
    let state = AppState::new(path, config)?;
    APP_STATE
        .set(state)
        .map_err(|_| anyhow::anyhow!("Application state is already initialized"))?;
    Ok(get())
}

pub fn init_for_pattern(pattern: &str, config: Config) -> Result<&'static AppState> {
    init(crate::helpers::watched_directory(pattern)?, config)
}

pub fn get() -> &'static AppState {
    APP_STATE
        .get()
        .expect("Application state is not initialized")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_and_absolute_paths_within_watched_directory() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let state = AppState::new(dir.path().to_path_buf(), Config::default())?;
        let file = state.path.join("test.txt");
        std::fs::write(&file, "test")?;

        assert_eq!(state.resolve_path("test.txt")?, file);
        assert_eq!(state.resolve_path(file.to_string_lossy().as_ref())?, file);
        Ok(())
    }

    #[test]
    fn rejects_paths_outside_watched_directory() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let state = AppState::new(dir.path().to_path_buf(), Config::default())?;

        let err = state
            .resolve_path("../outside.txt")
            .unwrap_err()
            .to_string();
        assert!(err.contains("outside watched directory"));
        Ok(())
    }
}
