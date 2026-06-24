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
