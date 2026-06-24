use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

use crate::linear::Issue;

/// Keys already used by the core UI; extensions configured to use these are skipped.
const RESERVED_KEYS: &[char] = &['q', 'j', 'k', 'o', 'r', 'f', 'l', 'h', 'c'];

#[derive(Debug, Clone, Deserialize)]
pub struct Extension {
    pub key: char,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize, Default)]
struct ExtensionsFile {
    #[serde(default)]
    extension: Vec<Extension>,
}

pub fn config_path() -> Result<PathBuf> {
    let home = directories::BaseDirs::new().context("could not determine home directory")?;
    Ok(home.home_dir().join(".harness").join("extensions.toml"))
}

/// Loads the extensions config, if present. Returns an empty list (not an error)
/// when the file doesn't exist, since extensions are optional. Entries bound to
/// reserved keys are dropped with a warning printed to stderr.
pub fn load() -> Result<Vec<Extension>> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read extensions config at {}", path.display()))?;
    let parsed: ExtensionsFile = toml::from_str(&text)
        .with_context(|| format!("failed to parse extensions config at {}", path.display()))?;

    let mut seen_keys = Vec::new();
    let mut extensions = Vec::new();
    for ext in parsed.extension {
        if RESERVED_KEYS.contains(&ext.key) {
            eprintln!(
                "warning: extension '{}' uses reserved key '{}', skipping",
                ext.name, ext.key
            );
            continue;
        }
        if seen_keys.contains(&ext.key) {
            eprintln!(
                "warning: extension '{}' reuses key '{}' already bound by another extension, skipping",
                ext.name, ext.key
            );
            continue;
        }
        seen_keys.push(ext.key);
        extensions.push(ext);
    }

    Ok(extensions)
}

fn build_command(template: &str, issue: &Issue, project_root: Option<&str>) -> String {
    template
        .replace("{identifier}", &issue.identifier)
        .replace("{title}", &issue.title)
        .replace("{url}", &issue.url)
        .replace("{team}", &issue.team.key)
        .replace("{state}", &issue.state.name)
        .replace("{priority}", &issue.priority.to_string())
        .replace(
            "{assignee}",
            issue.assignee.as_ref().map(|a| a.name.as_str()).unwrap_or(""),
        )
        .replace(
            "{project}",
            issue.project.as_ref().map(|p| p.name.as_str()).unwrap_or(""),
        )
        .replace("{project_root}", project_root.unwrap_or(""))
}

pub struct ExtensionRunResult {
    pub name: String,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs an extension's command against the given issue, substituting `{field}`
/// placeholders first (including `{project_root}`, from the active project
/// mapping, if any). The command is executed through the platform shell so
/// templates can use pipes, multiple args, etc. Only run extensions you wrote
/// or trust: issue fields (e.g. title) are interpolated directly into the
/// shell command string.
pub async fn run(
    extension: &Extension,
    issue: &Issue,
    project_root: Option<&str>,
) -> ExtensionRunResult {
    let cmd_str = build_command(&extension.command, issue, project_root);

    #[cfg(windows)]
    let mut command = {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(&cmd_str);
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(&cmd_str);
        c
    };

    match command.output().await {
        Ok(output) => ExtensionRunResult {
            name: extension.name.clone(),
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        },
        Err(e) => ExtensionRunResult {
            name: extension.name.clone(),
            success: false,
            stdout: String::new(),
            stderr: format!("failed to launch command: {e}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linear::{RelationConnection, State, Team};

    fn sample_issue() -> Issue {
        Issue {
            id: "id-1".to_string(),
            identifier: "SA-1".to_string(),
            title: "Fix the thing".to_string(),
            priority: 2.0,
            state: State {
                name: "Todo".to_string(),
                state_type: "unstarted".to_string(),
            },
            team: Team {
                name: "Staging Assistant".to_string(),
                key: "SA".to_string(),
            },
            project: None,
            updated_at: "2026-01-01T00:00:00.000Z".to_string(),
            url: "https://linear.app/x/issue/SA-1".to_string(),
            description: None,
            assignee: None,
            inverse_relations: RelationConnection { nodes: Vec::new() },
        }
    }

    #[test]
    fn substitutes_placeholders() {
        let cmd = build_command(
            "gen --id {identifier} --title \"{title}\"",
            &sample_issue(),
            None,
        );
        assert_eq!(cmd, "gen --id SA-1 --title \"Fix the thing\"");
    }

    #[test]
    fn substitutes_project_root() {
        let cmd = build_command(
            "cd {project_root} && run {identifier}",
            &sample_issue(),
            Some("/repos/va"),
        );
        assert_eq!(cmd, "cd /repos/va && run SA-1");
    }

    #[tokio::test]
    async fn runs_command_and_captures_stdout() {
        let extension = Extension {
            key: 'g',
            name: "Echo".to_string(),
            command: "echo hello {identifier}".to_string(),
            description: String::new(),
        };
        let result = run(&extension, &sample_issue(), None).await;
        assert!(result.success);
        assert!(result.stdout.contains("hello SA-1"));
    }

    #[test]
    fn skips_extensions_on_reserved_or_duplicate_keys() {
        let file = ExtensionsFile {
            extension: vec![
                Extension {
                    key: 'q',
                    name: "Bad".to_string(),
                    command: "echo".to_string(),
                    description: String::new(),
                },
                Extension {
                    key: 'g',
                    name: "Good".to_string(),
                    command: "echo".to_string(),
                    description: String::new(),
                },
                Extension {
                    key: 'g',
                    name: "Dup".to_string(),
                    command: "echo".to_string(),
                    description: String::new(),
                },
            ],
        };

        let mut seen_keys = Vec::new();
        let mut extensions = Vec::new();
        for ext in file.extension {
            if RESERVED_KEYS.contains(&ext.key) || seen_keys.contains(&ext.key) {
                continue;
            }
            seen_keys.push(ext.key);
            extensions.push(ext);
        }

        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].name, "Good");
    }
}
