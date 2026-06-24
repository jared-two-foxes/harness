use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Maps a local repo path to the Linear team/project it corresponds to, so
/// harness can default-filter the issue list and tell extension commands
/// where to run (via the `{project_root}` placeholder).
#[derive(Debug, Clone, Deserialize)]
pub struct Project {
    /// Repo path, e.g. "~/code/own/VirtualAssistant". `~` is expanded against
    /// the home directory.
    pub path: String,
    /// Linear team name, e.g. "staging_assistant".
    pub team: String,
    /// Linear project name, e.g. "backend".
    pub project: String,
}

impl Project {
    /// Absolute, `~`-expanded path. Used both for cwd matching and as the
    /// `{project_root}` value handed to extension commands.
    pub fn root(&self) -> PathBuf {
        expand_home(&self.path)
    }
}

#[derive(Debug, Deserialize, Default)]
struct ProjectsFile {
    #[serde(default)]
    project: Vec<Project>,
}

pub fn config_path() -> Result<PathBuf> {
    let home = directories::BaseDirs::new().context("could not determine home directory")?;
    Ok(home.home_dir().join(".harness").join("projects.toml"))
}

/// Loads the project mappings config, if present. Returns an empty list (not
/// an error) when the file doesn't exist, since project mapping is optional.
pub fn load() -> Result<Vec<Project>> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read projects config at {}", path.display()))?;
    let parsed: ProjectsFile = toml::from_str(&text)
        .with_context(|| format!("failed to parse projects config at {}", path.display()))?;
    Ok(parsed.project)
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        if let Some(home) = directories::BaseDirs::new() {
            return home.home_dir().join(rest);
        }
    }
    PathBuf::from(path)
}

/// Finds the mapping whose root is an ancestor of (or equal to) `cwd`,
/// preferring the most specific (longest) match.
pub fn find_active<'a>(projects: &'a [Project], cwd: &Path) -> Option<&'a Project> {
    projects
        .iter()
        .filter(|p| cwd.starts_with(p.root()))
        .max_by_key(|p| p.root().components().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(path: &str, team: &str, name: &str) -> Project {
        Project {
            path: path.to_string(),
            team: team.to_string(),
            project: name.to_string(),
        }
    }

    #[test]
    fn matches_descendant_of_project_root() {
        let projects = vec![project("/repos/va", "staging_assistant", "backend")];
        let cwd = PathBuf::from("/repos/va/libs/api");
        let active = find_active(&projects, &cwd).expect("should match");
        assert_eq!(active.team, "staging_assistant");
    }

    #[test]
    fn prefers_most_specific_match() {
        let projects = vec![
            project("/repos", "team-a", "proj-a"),
            project("/repos/va", "staging_assistant", "backend"),
        ];
        let cwd = PathBuf::from("/repos/va/libs/api");
        let active = find_active(&projects, &cwd).expect("should match");
        assert_eq!(active.team, "staging_assistant");
    }

    #[test]
    fn no_match_outside_any_project_root() {
        let projects = vec![project("/repos/va", "staging_assistant", "backend")];
        let cwd = PathBuf::from("/somewhere/else");
        assert!(find_active(&projects, &cwd).is_none());
    }
}
