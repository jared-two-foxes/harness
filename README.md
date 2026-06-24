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

| Key         | Action                          |
|-------------|----------------------------------|
| `j` / `↓`   | Move selection down              |
| `k` / `↑`   | Move selection up                |
| `t`         | Filter by team                   |
| `p`         | Filter by project                |
| `s`         | Filter by status (multi-select)  |
| `c`         | Clear all filters                |
| `r`         | Refresh issues from Linear       |
| `q` / `Esc` | Quit                             |

While a filter popup is open:

| Key             | Action                                  |
|-----------------|------------------------------------------|
| `j`/`k`         | Move cursor                              |
| `space`         | Toggle selection (status filter only)    |
| `Enter`         | Apply filter                             |
| `Esc`           | Cancel without changes                   |

## Features

- Lists all issues in the workspace, sorted by most recently updated
- Detail pane shows team, assignee, state, priority, and a markdown-rendered description
- Filter issues by team, project, and status (status supports selecting multiple values at once)
