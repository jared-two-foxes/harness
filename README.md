# harness

A terminal UI for browsing Linear issues, built with [ratatui](https://ratatui.rs). Eventually intended as an entry point into an AI code generation pipeline; for now it's a read-only Linear issue browser.

## Setup

The app needs a Linear API key. It looks for one in this order:

1. The `LINEAR_API_KEY` environment variable
2. `~/.secrets/linear-key`

## Running

```sh
cargo run
```

## Controls

### Issue list

| Key         | Action                          |
|-------------|----------------------------------|
| `j` / `↓`   | Move selection down              |
| `k` / `↑`   | Move selection up                |
| `Enter` / `l` | View selected issue's details  |
| `f`         | Open the filters menu             |
| `o`         | Toggle sort order (updated / priority) |
| `r`         | Refresh issues from Linear       |
| `q` / `Esc` | Quit                             |

### Detail view

| Key             | Action                                  |
|-----------------|------------------------------------------|
| `j` / `k`       | View next / previous issue               |
| `Esc` / `Enter` / `q` / `h` | Back to the issue list       |

### Filters menu (`f`)

One row per filter dimension (Team, Project, Status, Blocked), showing its current value.

| Key             | Action                                  |
|-----------------|------------------------------------------|
| `j` / `k`       | Move cursor                              |
| `Enter`         | Edit the highlighted filter              |
| `c`             | Clear all filters                        |
| `Esc`           | Close the menu                           |

Editing a single filter:

| Key             | Action                                                    |
|-----------------|------------------------------------------------------------|
| `j` / `k`       | Move cursor                                                |
| `space`         | Toggle selection (Status only — supports multiple values)  |
| `Enter`         | Apply and return to the filters menu                       |
| `Esc`           | Cancel and return to the filters menu                      |

## Features

- Lists all issues in the workspace, sorted by last-updated or priority
- Full-screen detail view with team, assignee, state, priority, blocked status, and a markdown-rendered description
- Filter issues by team, project, status (multi-select), and blocked state (any / unblocked only / blocked only), all from one consolidated filters menu
- Blocked issues (per Linear's issue relations) are marked with a `!` in the list and called out in the detail view
- Extensible: bind custom hotkeys to external commands (e.g. your own AI code-generation scripts) that run against the selected issue
- Project-aware: maps a local repo path to its Linear team/project so the issue list opens pre-scoped and extension commands know which repo to run in

## Extensions

Extensions let you bind a key to an external command that runs against the currently
selected issue — e.g. an `agentic` code-gen script that takes an issue identifier and
title and goes off to generate a PR. Harness shells out via `process::Command`
(`cmd /C` on Windows, `sh -c` elsewhere) and runs the command in the background so the
UI stays responsive; the captured stdout/stderr is shown in a results view when it's done.

Define extensions in `~/.harness/extensions.toml`:

```toml
[[extension]]
key = "g"
name = "Generate code"
command = "python3 ~/scripts/gen_code.py --issue {identifier} --title \"{title}\""
description = "Run the agentic code-gen pipeline for this issue"
```

Available placeholders, substituted from the selected issue before the command runs:

| Placeholder    | Value                                  |
|----------------|------------------------------------------|
| `{identifier}` | Issue identifier, e.g. `SA-123`          |
| `{title}`      | Issue title                              |
| `{url}`        | Linear URL for the issue                 |
| `{team}`       | Team key, e.g. `SA`                      |
| `{project}`    | Project name (empty if none)             |
| `{state}`      | Status name, e.g. `In Progress`          |
| `{priority}`   | Raw priority number (0–4)                |
| `{assignee}`   | Assignee name (empty if unassigned)      |
| `{project_root}` | Root path of the active project mapping (see below), empty if none |

Extensions can be triggered from the issue list or the detail view. Keys already used
by the core UI (`q j k o r f l h c`) are reserved — any extension bound to one of those,
or to a key another extension already claims, is skipped with a warning printed to
stderr on startup. Bound extensions show up in the footer alongside the built-in keys.

Since issue fields (like the title) are interpolated directly into a shell command
string, only configure extensions whose commands you trust — treat `extensions.toml`
like a script you'd run yourself.

## Project mapping

Since harness can run from any directory but a given repo usually corresponds to one
specific Linear team/project, you can teach it that mapping in `~/.harness/projects.toml`:

```toml
[[project]]
path = "~/code/own/VirtualAssistant"
team = "staging_assistant"
project = "backend"
```

`path` supports `~` for the home directory. At startup harness checks the current
working directory against every configured `path` (longest/most specific match wins)
and, if one matches:

- defaults the issue list's Team and Project filters to that mapping (still
  overridable afterwards via the filters menu — this is just the starting point,
  and it's reapplied on every `r` refresh)
- exposes the matched path to extensions as `{project_root}`, so a command like
  `cd /d "{project_root}" && python script.py {identifier}` runs against the right repo
  regardless of where harness itself was launched from

With no match (or no `projects.toml`), harness just shows every issue, unfiltered, as before.

### Example: binding the `check-ticket` / `resolve-ticket` pipeline scripts

Given Python scripts that expect to run with the target repo as their working
directory (e.g. they read/write `.gap-plan.md`, `.ticket.md` etc. relative to `cwd`),
`~/.harness/extensions.toml` can bind them like this:

```toml
[[extension]]
key = "x"
name = "Check ticket"
description = "Plan + report remaining acceptance criteria for this ticket"
command = 'cd /d "{project_root}" && python "C:/path/to/bin/check-ticket.py" {identifier}'

[[extension]]
key = "g"
name = "Resolve ticket"
description = "Run the TDD pipeline to implement this ticket's remaining criteria"
command = 'cd /d "{project_root}" && python "C:/path/to/bin/resolve-ticket.py" {identifier}'
```
