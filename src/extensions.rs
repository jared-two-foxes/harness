use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use crate::linear::Issue;

/// Keys already used by the core UI; extensions configured to use these are skipped.
const RESERVED_KEYS: &[char] = &[
    'q', 'j', 'k', 'o', 'r', 'f', 'l', 'h', 'c', 'd', 'u', 'g', 'G', 'K', 's', 'n',
];

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

/// One line of output as it arrives, or the run's final outcome.
pub enum ExtensionEvent {
    Line { name: String, stderr: bool, text: String },
    Done { name: String, success: bool },
}

/// Runs an extension's command against the given issue, substituting `{field}`
/// placeholders first (including `{project_root}`, from the active project
/// mapping, if any). The command is executed through the platform shell so
/// templates can use pipes, multiple args, etc. Only run extensions you wrote
/// or trust: issue fields (e.g. title) are interpolated directly into the
/// shell command string.
///
/// Output is streamed line-by-line over `tx` as it's produced, rather than
/// buffered until exit — important for long-running AI pipeline scripts,
/// where otherwise the UI would show nothing at all until the whole thing
/// finished. `cancel` lets the caller kill the child process early; either
/// way, exactly one `Done` event is sent once the process exits or is killed.
pub async fn run(
    extension: Extension,
    issue: Issue,
    project_root: Option<String>,
    tx: mpsc::UnboundedSender<ExtensionEvent>,
    mut cancel: oneshot::Receiver<()>,
) {
    let name = extension.name.clone();
    let cmd_str = build_command(&extension.command, &issue, project_root.as_deref());

    // On Windows, `arg()` would quote the whole command string as a single
    // CreateProcess argument (since it contains spaces), and that extra
    // layer of quoting collides with the command's own embedded quotes
    // (e.g. around `{project_root}`), corrupting paths passed to `cd /d`.
    // `raw_arg` appends text verbatim, since cmd.exe's `/C` doesn't follow
    // normal argv quoting rules anyway.
    #[cfg(windows)]
    let mut command = {
        use tokio::process::Command;
        let mut c = Command::new("cmd");
        c.raw_arg("/C").raw_arg(&cmd_str);
        c
    };
    // Run in its own process group so `kill_tree` can reach the whole tree
    // (the real work is a grandchild of this `sh -c`) via a negative PID,
    // rather than just this directly-tracked shell process.
    #[cfg(not(windows))]
    let mut command = {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(&cmd_str);
        c.process_group(0);
        c
    };

    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(ExtensionEvent::Line {
                name: name.clone(),
                stderr: true,
                text: format!("failed to launch command: {e}"),
            });
            let _ = tx.send(ExtensionEvent::Done { name, success: false });
            return;
        }
    };

    let mut stdout_lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let mut stderr_lines = BufReader::new(child.stderr.take().expect("piped stderr")).lines();
    let mut stdout_done = false;
    let mut stderr_done = false;

    let success = loop {
        tokio::select! {
            line = stdout_lines.next_line(), if !stdout_done => {
                match line {
                    Ok(Some(text)) => {
                        let _ = tx.send(ExtensionEvent::Line { name: name.clone(), stderr: false, text });
                    }
                    _ => stdout_done = true,
                }
            }
            line = stderr_lines.next_line(), if !stderr_done => {
                match line {
                    Ok(Some(text)) => {
                        let _ = tx.send(ExtensionEvent::Line { name: name.clone(), stderr: true, text });
                    }
                    _ => stderr_done = true,
                }
            }
            status = child.wait() => {
                break status.map(|s| s.success()).unwrap_or(false);
            }
            _ = &mut cancel => {
                kill_tree(&mut child).await;
                let _ = tx.send(ExtensionEvent::Line {
                    name: name.clone(),
                    stderr: true,
                    text: "(cancelled by user)".to_string(),
                });
                break false;
            }
        }
    };

    let _ = tx.send(ExtensionEvent::Done { name, success });
}

/// Kills `child` and its descendants. The tracked child is always `cmd /C`
/// (Windows) or `sh -c` (Unix) wrapping the real command, so the actual
/// work (e.g. a Python script, and anything *it* spawns) runs as a
/// grandchild — killing just the tracked process leaves it running. On
/// Windows, `taskkill /T` kills the whole tree by PID; on Unix the child
/// runs in its own process group (see `process_group(0)` below) so a
/// negative-PID `kill` reaches the whole group.
async fn kill_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        #[cfg(windows)]
        {
            let _ = tokio::process::Command::new("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .output()
                .await;
        }
        #[cfg(not(windows))]
        {
            let _ = tokio::process::Command::new("kill")
                .args(["-9", &format!("-{pid}")])
                .output()
                .await;
        }
    }
    // Reap the tracked child and make sure it's gone even if the above failed
    // (e.g. it had already exited on its own).
    let _ = child.kill().await;
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
                id: "state-1".to_string(),
                name: "Todo".to_string(),
                state_type: "unstarted".to_string(),
            },
            team: Team {
                id: "team-1".to_string(),
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

    /// Drives `run()` to completion and collects every event it sent, for
    /// tests that don't care about interleaving with other UI activity.
    async fn run_and_collect(extension: Extension, issue: Issue) -> (Vec<String>, bool) {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_cancel_tx, cancel_rx) = oneshot::channel();
        run(extension, issue, None, tx, cancel_rx).await;

        let mut lines = Vec::new();
        let mut success = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                ExtensionEvent::Line { text, .. } => lines.push(text),
                ExtensionEvent::Done { success: s, .. } => success = s,
            }
        }
        (lines, success)
    }

    #[tokio::test]
    async fn runs_command_and_streams_stdout() {
        let extension = Extension {
            key: 'g',
            name: "Echo".to_string(),
            command: "echo hello {identifier}".to_string(),
            description: String::new(),
        };
        let (lines, success) = run_and_collect(extension, sample_issue()).await;
        assert!(success);
        assert!(lines.iter().any(|l| l.contains("hello SA-1")));
    }

    /// Regression test: a command with `cd` into a quoted path plus other
    /// quoted args used to fail on Windows ("The filename, directory name,
    /// or volume label syntax is incorrect") because `arg()` wrapped the
    /// whole already-quoted command string in another layer of quoting.
    #[cfg(windows)]
    #[tokio::test]
    async fn runs_quoted_cd_command_without_corrupting_path() {
        let dir = std::env::temp_dir();
        let dir_str = dir.display().to_string();
        let extension = Extension {
            key: 'g',
            name: "CdTest".to_string(),
            command: format!(r#"cd /d "{}" && echo ok {{identifier}}"#, dir_str),
            description: String::new(),
        };
        let (lines, success) = run_and_collect(extension, sample_issue()).await;
        assert!(success, "lines={lines:?}");
        assert!(lines.iter().any(|l| l.contains("ok SA-1")));
    }

    #[tokio::test]
    async fn cancelling_kills_the_process_and_reports_failure() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (cancel_tx, cancel_rx) = oneshot::channel();

        // A command that would run far longer than the test should wait.
        #[cfg(windows)]
        let command = "ping -n 30 127.0.0.1 >NUL".to_string();
        #[cfg(not(windows))]
        let command = "sleep 30".to_string();

        let extension = Extension {
            key: 'g',
            name: "Slow".to_string(),
            command,
            description: String::new(),
        };

        let t0 = std::time::Instant::now();
        let handle = tokio::spawn(run(extension, sample_issue(), None, tx, cancel_rx));
        // Give the process a moment to actually start before killing it.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let _ = cancel_tx.send(());
        handle.await.expect("run task should not panic");

        // Regression check: an earlier version of `kill_tree` only killed the
        // directly-tracked `cmd /C` / `sh -c` process, leaving the real
        // grandchild (here, `ping`/`sleep`) running for its full 30s — which
        // also meant a real extension's actual script kept running after
        // "kill" was pressed. `kill_tree` killing the whole tree should let
        // this finish in well under a second.
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(5),
            "cancelling took {:?} — the process tree likely wasn't fully killed",
            t0.elapsed()
        );

        let mut success = true;
        while let Ok(event) = rx.try_recv() {
            if let ExtensionEvent::Done { success: s, .. } = event {
                success = s;
            }
        }
        assert!(!success, "a cancelled run should report failure");
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
                    key: 'x',
                    name: "Good".to_string(),
                    command: "echo".to_string(),
                    description: String::new(),
                },
                Extension {
                    key: 'x',
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
