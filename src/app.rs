use crate::extensions::{Extension, ExtensionRunResult};
use crate::linear::Issue;
use crate::project::Project;

pub enum LoadState {
    Loading,
    Loaded,
    Error(String),
}

/// All filterable dimensions, in the order they appear in the filter menu.
pub const FILTER_KINDS: [FilterKind; 4] = [
    FilterKind::Team,
    FilterKind::Project,
    FilterKind::Status,
    FilterKind::Blocked,
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Team,
    Project,
    Status,
    Blocked,
}

impl FilterKind {
    pub fn label(self) -> &'static str {
        match self {
            FilterKind::Team => "Team",
            FilterKind::Project => "Project",
            FilterKind::Status => "Status",
            FilterKind::Blocked => "Blocked",
        }
    }

    /// Multi-select dimensions show checkboxes and accumulate values;
    /// single-select dimensions pick exactly one value (or the default).
    pub fn is_multi(self) -> bool {
        matches!(self, FilterKind::Status)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BlockedFilter {
    Any,
    UnblockedOnly,
    BlockedOnly,
}

impl BlockedFilter {
    const OPTIONS: [&'static str; 3] = ["Any", "Unblocked only", "Blocked only"];

    fn label(self) -> &'static str {
        Self::OPTIONS[self as usize]
    }

    fn from_index(i: usize) -> Self {
        match i {
            1 => BlockedFilter::UnblockedOnly,
            2 => BlockedFilter::BlockedOnly,
            _ => BlockedFilter::Any,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Updated,
    Priority,
}

impl SortKey {
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Updated => "updated",
            SortKey::Priority => "priority",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            SortKey::Updated => SortKey::Priority,
            SortKey::Priority => SortKey::Updated,
        }
    }
}

pub enum Mode {
    Normal,
    /// Top-level "Filters" menu: one row per FilterKind, showing its current value.
    FilterMenu {
        selected: usize,
    },
    /// Editing a single filter dimension's value(s).
    Filter {
        kind: FilterKind,
        options: Vec<String>,
        selected: usize,
        checked: Vec<bool>,
    },
    /// Viewing the selected issue's full details.
    Detail,
    /// Viewing the output of a running or completed extension command.
    ExtensionOutput {
        name: String,
        running: bool,
        success: bool,
        stdout: String,
        stderr: String,
        /// Current scroll offset (in lines) into the rendered output.
        scroll: u16,
        /// Total rendered line count, for clamping `scroll`.
        line_count: u16,
    },
}

#[derive(Default)]
pub struct Filters {
    pub team: Option<String>,
    pub project: Option<String>,
    pub status: Vec<String>,
    pub blocked: Option<BlockedFilter>,
}

impl Filters {
    fn blocked(&self) -> BlockedFilter {
        self.blocked.unwrap_or(BlockedFilter::Any)
    }

    pub fn is_active(&self, kind: FilterKind) -> bool {
        match kind {
            FilterKind::Team => self.team.is_some(),
            FilterKind::Project => self.project.is_some(),
            FilterKind::Status => !self.status.is_empty(),
            FilterKind::Blocked => self.blocked() != BlockedFilter::Any,
        }
    }

    pub fn summary(&self, kind: FilterKind) -> String {
        match kind {
            FilterKind::Team => self.team.clone().unwrap_or_else(|| "All".to_string()),
            FilterKind::Project => self.project.clone().unwrap_or_else(|| "All".to_string()),
            FilterKind::Status => {
                if self.status.is_empty() {
                    "All".to_string()
                } else {
                    self.status.join(", ")
                }
            }
            FilterKind::Blocked => self.blocked().label().to_string(),
        }
    }

    fn clear(&mut self) {
        self.team = None;
        self.project = None;
        self.status.clear();
        self.blocked = None;
    }

    fn matches(&self, issue: &Issue) -> bool {
        self.team.as_ref().map_or(true, |t| &issue.team.name == t)
            && self.project.as_ref().map_or(true, |p| {
                issue
                    .project
                    .as_ref()
                    .map(|proj| &proj.name == p)
                    .unwrap_or(false)
            })
            && (self.status.is_empty() || self.status.iter().any(|s| s == &issue.state.name))
            && match self.blocked() {
                BlockedFilter::Any => true,
                BlockedFilter::UnblockedOnly => !issue.is_blocked(),
                BlockedFilter::BlockedOnly => issue.is_blocked(),
            }
    }
}

pub struct App {
    pub all_issues: Vec<Issue>,
    pub issues: Vec<Issue>,
    pub selected: usize,
    pub load_state: LoadState,
    pub should_quit: bool,
    pub filters: Filters,
    pub sort_key: SortKey,
    pub mode: Mode,
    pub extensions: Vec<Extension>,
    pub active_project: Option<Project>,
}

const ALL_LABEL: &str = "(All)";

/// Mirrors the line count `ui::draw_extension_output` renders, so scroll can
/// be clamped without the UI layer having to report it back.
fn output_line_count(stdout: &str, stderr: &str) -> u16 {
    let mut total = 0u16;
    if !stdout.is_empty() {
        total += 1 + stdout.lines().count() as u16;
    }
    if !stderr.is_empty() {
        if total > 0 {
            total += 1;
        }
        total += 1 + stderr.lines().count() as u16;
    }
    if stdout.is_empty() && stderr.is_empty() {
        total += 1;
    }
    total
}

impl App {
    pub fn new() -> Self {
        Self {
            all_issues: Vec::new(),
            issues: Vec::new(),
            selected: 0,
            load_state: LoadState::Loading,
            should_quit: false,
            filters: Filters::default(),
            sort_key: SortKey::Updated,
            mode: Mode::Normal,
            extensions: Vec::new(),
            active_project: None,
        }
    }

    /// Sets the active project mapping and defaults the team/project filters
    /// to it, so the issue list opens already scoped to the right Linear
    /// team/project for the repo harness was launched from. Still
    /// user-overridable afterwards via the filters menu.
    pub fn set_active_project(&mut self, project: Project) {
        self.filters.team = Some(project.team.clone());
        self.filters.project = Some(project.project.clone());
        self.active_project = Some(project);
        self.apply_filters();
    }

    /// The active project's root path, for the `{project_root}` extension
    /// placeholder. `None` when harness wasn't launched inside a mapped repo.
    pub fn project_root(&self) -> Option<String> {
        self.active_project
            .as_ref()
            .map(|p| p.root().display().to_string())
    }

    pub fn set_issues(&mut self, issues: Vec<Issue>) {
        self.all_issues = issues;
        self.filters.clear();
        // Re-scope to the active project's team/project on every (re)load,
        // including refreshes, since that default isn't something a refresh
        // should silently drop.
        if let Some(project) = &self.active_project {
            self.filters.team = Some(project.team.clone());
            self.filters.project = Some(project.project.clone());
        }
        self.load_state = LoadState::Loaded;
        self.apply_filters();
    }

    pub fn toggle_sort(&mut self) {
        self.sort_key = self.sort_key.toggled();
        self.apply_filters();
    }

    pub fn set_error(&mut self, msg: String) {
        self.load_state = LoadState::Error(msg);
    }

    pub fn selected_issue(&self) -> Option<&Issue> {
        self.issues.get(self.selected)
    }

    pub fn select_next(&mut self) {
        if !self.issues.is_empty() {
            self.selected = (self.selected + 1).min(self.issues.len() - 1);
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn apply_filters(&mut self) {
        self.issues = self
            .all_issues
            .iter()
            .filter(|issue| self.filters.matches(issue))
            .cloned()
            .collect();

        match self.sort_key {
            SortKey::Updated => self.issues.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
            SortKey::Priority => self
                .issues
                .sort_by_key(|i| (i.priority_rank(), i.identifier.clone())),
        }

        self.selected = 0;
    }

    pub fn clear_filters(&mut self) {
        self.filters.clear();
        self.apply_filters();
    }

    pub fn open_detail(&mut self) {
        if self.selected_issue().is_some() {
            self.mode = Mode::Detail;
        }
    }

    pub fn close_detail(&mut self) {
        self.mode = Mode::Normal;
    }

    /// Looks up the extension bound to a key, if any. Usable from any mode that
    /// allows acting on the selected issue (Normal, Detail).
    pub fn find_extension(&self, key: char) -> Option<Extension> {
        self.extensions.iter().find(|e| e.key == key).cloned()
    }

    pub fn start_extension(&mut self, name: String) {
        self.mode = Mode::ExtensionOutput {
            name,
            running: true,
            success: false,
            stdout: String::new(),
            stderr: String::new(),
            scroll: 0,
            line_count: 0,
        };
    }

    pub fn finish_extension(&mut self, result: ExtensionRunResult) {
        // Ignore results for an extension run the user has already navigated away from.
        if let Mode::ExtensionOutput { name, running, .. } = &self.mode {
            if *running && *name == result.name {
                let line_count = output_line_count(&result.stdout, &result.stderr);
                self.mode = Mode::ExtensionOutput {
                    name: result.name,
                    running: false,
                    success: result.success,
                    stdout: result.stdout,
                    stderr: result.stderr,
                    scroll: 0,
                    line_count,
                };
            }
        }
    }

    pub fn close_extension_output(&mut self) {
        self.mode = Mode::Normal;
    }

    /// Scrolls the extension output view by `delta` lines (negative scrolls up),
    /// clamped to the rendered content's line count.
    pub fn scroll_extension_output(&mut self, delta: i32) {
        if let Mode::ExtensionOutput {
            running,
            scroll,
            line_count,
            ..
        } = &mut self.mode
        {
            if !*running {
                let max = *line_count as i64;
                *scroll = (*scroll as i64 + delta as i64).clamp(0, max) as u16;
            }
        }
    }

    pub fn open_filter_menu(&mut self) {
        self.mode = Mode::FilterMenu { selected: 0 };
    }

    pub fn filter_menu_move(&mut self, delta: i32) {
        if let Mode::FilterMenu { selected } = &mut self.mode {
            let len = FILTER_KINDS.len() as i32;
            *selected = ((*selected as i32 + delta).rem_euclid(len)) as usize;
        }
    }

    pub fn filter_menu_select(&mut self) {
        if let Mode::FilterMenu { selected } = &self.mode {
            match FILTER_KINDS[*selected] {
                FilterKind::Team => self.open_value_filter(FilterKind::Team),
                FilterKind::Project => self.open_value_filter(FilterKind::Project),
                FilterKind::Status => self.open_value_filter(FilterKind::Status),
                FilterKind::Blocked => self.open_value_filter(FilterKind::Blocked),
            }
        }
    }

    pub fn filter_menu_cancel(&mut self) {
        self.mode = Mode::Normal;
    }

    fn open_value_filter(&mut self, kind: FilterKind) {
        let (options, selected, checked) = match kind {
            FilterKind::Team => {
                let mut names: Vec<String> =
                    self.all_issues.iter().map(|i| i.team.name.clone()).collect();
                names.sort();
                names.dedup();
                let mut options = vec![ALL_LABEL.to_string()];
                options.extend(names);
                let selected = self
                    .filters
                    .team
                    .as_ref()
                    .and_then(|t| options.iter().position(|o| o == t))
                    .unwrap_or(0);
                let checked = vec![false; options.len()];
                (options, selected, checked)
            }
            FilterKind::Project => {
                let mut names: Vec<String> = self
                    .all_issues
                    .iter()
                    .filter_map(|i| i.project.as_ref().map(|p| p.name.clone()))
                    .collect();
                names.sort();
                names.dedup();
                let mut options = vec![ALL_LABEL.to_string()];
                options.extend(names);
                let selected = self
                    .filters
                    .project
                    .as_ref()
                    .and_then(|p| options.iter().position(|o| o == p))
                    .unwrap_or(0);
                let checked = vec![false; options.len()];
                (options, selected, checked)
            }
            FilterKind::Status => {
                let mut names: Vec<String> =
                    self.all_issues.iter().map(|i| i.state.name.clone()).collect();
                names.sort();
                names.dedup();
                let checked: Vec<bool> = names
                    .iter()
                    .map(|o| self.filters.status.iter().any(|s| s == o))
                    .collect();
                (names, 0, checked)
            }
            FilterKind::Blocked => {
                let options: Vec<String> =
                    BlockedFilter::OPTIONS.iter().map(|s| s.to_string()).collect();
                let selected = match self.filters.blocked() {
                    BlockedFilter::Any => 0,
                    BlockedFilter::UnblockedOnly => 1,
                    BlockedFilter::BlockedOnly => 2,
                };
                let checked = vec![false; options.len()];
                (options, selected, checked)
            }
        };

        self.mode = Mode::Filter {
            kind,
            options,
            selected,
            checked,
        };
    }

    pub fn filter_move(&mut self, delta: i32) {
        if let Mode::Filter {
            options, selected, ..
        } = &mut self.mode
        {
            let len = options.len() as i32;
            if len > 0 {
                *selected = ((*selected as i32 + delta).rem_euclid(len)) as usize;
            }
        }
    }

    pub fn filter_toggle(&mut self) {
        if let Mode::Filter {
            kind,
            selected,
            checked,
            ..
        } = &mut self.mode
        {
            if kind.is_multi() {
                if let Some(c) = checked.get_mut(*selected) {
                    *c = !*c;
                }
            }
        }
    }

    pub fn filter_confirm(&mut self) {
        let mut return_to = 0;
        if let Mode::Filter {
            kind,
            options,
            selected,
            checked,
        } = &self.mode
        {
            return_to = FILTER_KINDS.iter().position(|k| k == kind).unwrap_or(0);
            if kind.is_multi() {
                let chosen: Vec<String> = options
                    .iter()
                    .zip(checked.iter())
                    .filter(|(_, &c)| c)
                    .map(|(o, _)| o.clone())
                    .collect();
                if *kind == FilterKind::Status {
                    self.filters.status = chosen;
                }
            } else {
                let value = options.get(*selected).cloned().filter(|c| c != ALL_LABEL);
                match kind {
                    FilterKind::Team => self.filters.team = value,
                    FilterKind::Project => self.filters.project = value,
                    FilterKind::Blocked => {
                        self.filters.blocked = Some(BlockedFilter::from_index(*selected))
                    }
                    FilterKind::Status => {}
                }
            }
        }
        self.mode = Mode::FilterMenu {
            selected: return_to,
        };
        self.apply_filters();
    }

    pub fn filter_cancel(&mut self) {
        if let Mode::Filter { kind, .. } = &self.mode {
            let idx = FILTER_KINDS.iter().position(|k| k == kind).unwrap_or(0);
            self.mode = Mode::FilterMenu { selected: idx };
        } else {
            self.mode = Mode::Normal;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::ExtensionRunResult;

    fn finished_output(stdout: &str, stderr: &str) -> App {
        let mut app = App::new();
        app.start_extension("Test".to_string());
        app.finish_extension(ExtensionRunResult {
            name: "Test".to_string(),
            success: true,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        });
        app
    }

    #[test]
    fn scroll_clamps_to_zero_minimum() {
        let mut app = finished_output("line1\nline2\nline3", "");
        app.scroll_extension_output(-100);
        let Mode::ExtensionOutput { scroll, .. } = app.mode else {
            panic!("expected ExtensionOutput mode");
        };
        assert_eq!(scroll, 0);
    }

    #[test]
    fn scroll_clamps_to_line_count_maximum() {
        let mut app = finished_output("line1\nline2\nline3", "");
        app.scroll_extension_output(i32::MAX);
        let Mode::ExtensionOutput {
            scroll, line_count, ..
        } = app.mode
        else {
            panic!("expected ExtensionOutput mode");
        };
        assert_eq!(scroll, line_count);
        assert!(scroll > 0);
    }

    #[test]
    fn scroll_does_nothing_while_running() {
        let mut app = App::new();
        app.start_extension("Test".to_string());
        app.scroll_extension_output(5);
        let Mode::ExtensionOutput { scroll, .. } = app.mode else {
            panic!("expected ExtensionOutput mode");
        };
        assert_eq!(scroll, 0);
    }
}
