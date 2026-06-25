use tokio::sync::oneshot;

use crate::extensions::Extension;
use crate::linear::{Issue, State, WorkflowState};
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
    /// Viewing the output of a running or completed extension command. The
    /// run itself lives in `App::extension_run`, independent of this mode,
    /// so navigating away (and back via `show_extension_output`) doesn't
    /// lose anything that arrives in the meantime.
    ExtensionOutput {
        /// Current scroll offset (in lines) into the rendered output.
        scroll: u16,
    },
    /// Picking a new status for the selected issue, from its team's
    /// available workflow states.
    StatusPicker {
        options: Vec<WorkflowState>,
        selected: usize,
    },
    /// Typing a title for a brand-new issue, created against the active
    /// project's team/project mapping.
    NewIssueTitle {
        input: String,
    },
}

/// A single extension invocation's lifecycle: lines accumulate as they
/// arrive from the child process's stdout/stderr, independent of whether the
/// output view is currently on screen, so results are never silently lost by
/// navigating away mid-run. `cancel` sends a kill signal to the running
/// process when present (i.e. while still running).
pub struct ExtensionRun {
    pub name: String,
    pub running: bool,
    pub success: bool,
    pub lines: Vec<(bool /* is_stderr */, String)>,
    cancel: Option<oneshot::Sender<()>>,
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
    pub detail_scroll: u16,
    pub extension_run: Option<ExtensionRun>,
}

const ALL_LABEL: &str = "(All)";

/// Fixed metadata line count rendered above the description in the detail view.
const DETAIL_HEADER_LINES: u16 = 10;

/// Approximate total rendered line count for the detail view (see `scroll_detail`).
fn detail_line_count(issue: &Issue) -> u16 {
    match &issue.description {
        Some(d) if !d.is_empty() => DETAIL_HEADER_LINES + 2 + d.lines().count() as u16,
        _ => DETAIL_HEADER_LINES,
    }
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
            detail_scroll: 0,
            extension_run: None,
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
            self.detail_scroll = 0;
        }
    }

    pub fn close_detail(&mut self) {
        self.mode = Mode::Normal;
    }

    /// Scrolls the detail view by `delta` lines (negative scrolls up), clamped
    /// to an approximate line count for the selected issue's content (header
    /// fields plus its rendered description, ignoring wrap — generous enough
    /// to avoid scrolling wildly past the end without needing the UI layer's
    /// exact wrapped line count).
    pub fn scroll_detail(&mut self, delta: i32) {
        if !matches!(self.mode, Mode::Detail) {
            return;
        }
        let max = self.selected_issue().map(detail_line_count).unwrap_or(0) as i64;
        self.detail_scroll = (self.detail_scroll as i64 + delta as i64).clamp(0, max) as u16;
    }

    /// Looks up the extension bound to a key, if any. Usable from any mode that
    /// allows acting on the selected issue (Normal, Detail).
    pub fn find_extension(&self, key: char) -> Option<Extension> {
        self.extensions.iter().find(|e| e.key == key).cloned()
    }

    /// True while an extension command is still running in the background,
    /// regardless of whether its output view is the one currently on screen.
    pub fn extension_running(&self) -> bool {
        self.extension_run.as_ref().is_some_and(|r| r.running)
    }

    /// Starts tracking a new extension run and switches to its output view.
    /// Replaces any previous (necessarily finished — see `extension_running`)
    /// run's record.
    pub fn start_extension(&mut self, name: String, cancel: oneshot::Sender<()>) {
        self.extension_run = Some(ExtensionRun {
            name,
            running: true,
            success: false,
            lines: Vec::new(),
            cancel: Some(cancel),
        });
        self.show_extension_output();
    }

    /// Switches to the extension output view without starting a new run —
    /// used both right after starting one and to return to an in-progress
    /// or already-finished run's output after navigating away.
    pub fn show_extension_output(&mut self) {
        self.mode = Mode::ExtensionOutput { scroll: 0 };
    }

    /// Appends one line of output from the named run, if it's still the
    /// current one (defensive: in practice only one run exists at a time).
    pub fn push_extension_line(&mut self, name: &str, is_stderr: bool, text: String) {
        if let Some(run) = &mut self.extension_run {
            if run.name == name {
                run.lines.push((is_stderr, text));
            }
        }
    }

    /// Marks the named run finished, if it's still the current one.
    pub fn finish_extension_run(&mut self, name: &str, success: bool) {
        if let Some(run) = &mut self.extension_run {
            if run.name == name {
                run.running = false;
                run.success = success;
                run.cancel = None;
            }
        }
    }

    /// Sends a kill signal to the currently running extension, if any.
    pub fn cancel_running_extension(&mut self) {
        if let Some(run) = &mut self.extension_run {
            if let Some(cancel) = run.cancel.take() {
                let _ = cancel.send(());
            }
        }
    }

    /// Just hides the output view — the run (if still going) keeps going in
    /// the background and can be reopened with `show_extension_output`.
    pub fn close_extension_output(&mut self) {
        self.mode = Mode::Normal;
    }

    /// Scrolls the extension output view by `delta` lines (negative scrolls
    /// up), clamped to the current line count. Allowed while running too,
    /// since output accumulates live and the user may want to scroll back
    /// through earlier lines without waiting for completion.
    pub fn scroll_extension_output(&mut self, delta: i32) {
        if !matches!(self.mode, Mode::ExtensionOutput { .. }) {
            return;
        }
        let max = self
            .extension_run
            .as_ref()
            .map(|r| r.lines.len())
            .unwrap_or(0) as i64;
        if let Mode::ExtensionOutput { scroll } = &mut self.mode {
            *scroll = (*scroll as i64 + delta as i64).clamp(0, max) as u16;
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

    /// Opens the status picker for the selected issue, pre-selecting its
    /// current status if it's among the given options.
    pub fn open_status_picker(&mut self, options: Vec<WorkflowState>) {
        let Some(issue) = self.selected_issue() else {
            return;
        };
        let selected = options
            .iter()
            .position(|s| s.id == issue.state.id)
            .unwrap_or(0);
        self.mode = Mode::StatusPicker { options, selected };
    }

    pub fn status_picker_move(&mut self, delta: i32) {
        if let Mode::StatusPicker { options, selected } = &mut self.mode {
            let len = options.len() as i32;
            if len > 0 {
                *selected = ((*selected as i32 + delta).rem_euclid(len)) as usize;
            }
        }
    }

    pub fn status_picker_cancel(&mut self) {
        self.mode = Mode::Normal;
    }

    /// Closes the picker and, if a selection was made, returns the selected
    /// issue's id plus the chosen status for the caller to submit to the API.
    pub fn status_picker_confirm(&mut self) -> Option<(String, WorkflowState)> {
        let result = if let Mode::StatusPicker { options, selected } = &self.mode {
            self.selected_issue()
                .map(|issue| (issue.id.clone(), options[*selected].clone()))
        } else {
            None
        };
        self.mode = Mode::Normal;
        result
    }

    /// Applies a status change that's already succeeded against the API, to
    /// both the unfiltered and (after re-filtering) the visible issue list.
    /// Re-running the filters matters since the new status may no longer
    /// match an active status filter; the previously-selected issue stays
    /// selected if it's still visible afterwards.
    pub fn apply_state_change(&mut self, issue_id: &str, new_state: WorkflowState) {
        let current_id = self.selected_issue().map(|i| i.id.clone());
        if let Some(issue) = self.all_issues.iter_mut().find(|i| i.id == issue_id) {
            issue.state = State {
                id: new_state.id,
                name: new_state.name,
                state_type: new_state.state_type,
            };
        }
        self.apply_filters();
        if let Some(id) = current_id {
            if let Some(pos) = self.issues.iter().position(|i| i.id == id) {
                self.selected = pos;
            }
        }
    }

    /// Opens the new-issue title prompt. Only meaningful when there's an
    /// active project mapping, since that's what supplies the team/project
    /// the new issue gets created against.
    pub fn open_new_issue(&mut self) {
        self.mode = Mode::NewIssueTitle {
            input: String::new(),
        };
    }

    pub fn new_issue_input(&mut self, c: char) {
        if let Mode::NewIssueTitle { input } = &mut self.mode {
            input.push(c);
        }
    }

    pub fn new_issue_backspace(&mut self) {
        if let Mode::NewIssueTitle { input } = &mut self.mode {
            input.pop();
        }
    }

    pub fn new_issue_cancel(&mut self) {
        self.mode = Mode::Normal;
    }

    /// Closes the prompt and, if a non-empty title was entered, returns it
    /// for the caller to submit to the API.
    pub fn new_issue_confirm(&mut self) -> Option<String> {
        let title = if let Mode::NewIssueTitle { input } = &self.mode {
            let trimmed = input.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        } else {
            None
        };
        self.mode = Mode::Normal;
        title
    }

    /// Inserts a newly created issue (already confirmed against the API)
    /// into the list, re-applies filters, and selects it.
    pub fn add_issue(&mut self, issue: Issue) {
        let id = issue.id.clone();
        self.all_issues.push(issue);
        self.apply_filters();
        if let Some(pos) = self.issues.iter().position(|i| i.id == id) {
            self.selected = pos;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linear::{Assignee, RelationConnection, Team};

    fn sample_issue(id: &str, identifier: &str, state_id: &str, state_name: &str) -> Issue {
        Issue {
            id: id.to_string(),
            identifier: identifier.to_string(),
            title: format!("Issue {identifier}"),
            priority: 0.0,
            state: State {
                id: state_id.to_string(),
                name: state_name.to_string(),
                state_type: "unstarted".to_string(),
            },
            team: Team {
                id: "team-1".to_string(),
                name: "Staging Assistant".to_string(),
                key: "SA".to_string(),
            },
            project: None,
            updated_at: "2026-01-01T00:00:00.000Z".to_string(),
            url: "https://linear.app/x/issue/1".to_string(),
            description: None,
            assignee: None::<Assignee>,
            inverse_relations: RelationConnection { nodes: Vec::new() },
        }
    }

    fn workflow_state(id: &str, name: &str) -> WorkflowState {
        WorkflowState {
            id: id.to_string(),
            name: name.to_string(),
            state_type: "started".to_string(),
            position: 0.0,
        }
    }

    #[test]
    fn status_picker_preselects_the_issues_current_state() {
        let mut app = App::new();
        app.set_issues(vec![sample_issue("1", "SA-1", "state-todo", "Todo")]);
        let options = vec![
            workflow_state("state-todo", "Todo"),
            workflow_state("state-done", "Done"),
        ];
        app.open_status_picker(options);
        match app.mode {
            Mode::StatusPicker { selected, .. } => assert_eq!(selected, 0),
            _ => panic!("expected StatusPicker mode"),
        }
    }

    #[test]
    fn confirming_status_picker_returns_issue_id_and_chosen_state() {
        let mut app = App::new();
        app.set_issues(vec![sample_issue("1", "SA-1", "state-todo", "Todo")]);
        app.open_status_picker(vec![
            workflow_state("state-todo", "Todo"),
            workflow_state("state-done", "Done"),
        ]);
        app.status_picker_move(1);
        let (issue_id, new_state) = app.status_picker_confirm().expect("a selection");
        assert_eq!(issue_id, "1");
        assert_eq!(new_state.name, "Done");
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[test]
    fn apply_state_change_updates_the_issue_and_keeps_it_selected() {
        let mut app = App::new();
        app.set_issues(vec![
            sample_issue("1", "SA-1", "state-todo", "Todo"),
            sample_issue("2", "SA-2", "state-todo", "Todo"),
        ]);
        app.selected = 1;
        app.apply_state_change("2", workflow_state("state-done", "Done"));

        let updated = app.all_issues.iter().find(|i| i.id == "2").unwrap();
        assert_eq!(updated.state.name, "Done");
        assert_eq!(app.selected_issue().unwrap().id, "2");
    }

    #[test]
    fn apply_state_change_drops_issue_out_of_view_when_filtered_out() {
        let mut app = App::new();
        app.set_issues(vec![sample_issue("1", "SA-1", "state-todo", "Todo")]);
        app.filters.status = vec!["Todo".to_string()];
        app.apply_filters();
        assert_eq!(app.issues.len(), 1);

        app.apply_state_change("1", workflow_state("state-done", "Done"));
        assert!(app.issues.is_empty(), "issue should be filtered out once its status no longer matches");
    }

    #[test]
    fn new_issue_prompt_collects_typed_title_and_trims_it() {
        let mut app = App::new();
        app.open_new_issue();
        for c in "  Fix the bug  ".chars() {
            app.new_issue_input(c);
        }
        let title = app.new_issue_confirm().expect("non-empty title");
        assert_eq!(title, "Fix the bug");
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[test]
    fn new_issue_prompt_rejects_blank_title() {
        let mut app = App::new();
        app.open_new_issue();
        for c in "   ".chars() {
            app.new_issue_input(c);
        }
        assert!(app.new_issue_confirm().is_none());
    }

    #[test]
    fn backspace_removes_last_character_of_new_issue_title() {
        let mut app = App::new();
        app.open_new_issue();
        app.new_issue_input('a');
        app.new_issue_input('b');
        app.new_issue_backspace();
        match &app.mode {
            Mode::NewIssueTitle { input } => assert_eq!(input, "a"),
            _ => panic!("expected NewIssueTitle mode"),
        }
    }

    #[test]
    fn add_issue_inserts_and_selects_the_new_issue() {
        let mut app = App::new();
        app.set_issues(vec![sample_issue("1", "SA-1", "state-todo", "Todo")]);
        app.add_issue(sample_issue("2", "SA-2", "state-todo", "Todo"));
        assert_eq!(app.all_issues.len(), 2);
        assert_eq!(app.selected_issue().unwrap().id, "2");
    }

    fn app_with_lines(lines: &[&str]) -> App {
        let mut app = App::new();
        let (cancel_tx, _cancel_rx) = oneshot::channel();
        app.start_extension("Test".to_string(), cancel_tx);
        for line in lines {
            app.push_extension_line("Test", false, line.to_string());
        }
        app.finish_extension_run("Test", true);
        app
    }

    fn scroll(app: &App) -> u16 {
        match app.mode {
            Mode::ExtensionOutput { scroll } => scroll,
            _ => panic!("expected ExtensionOutput mode"),
        }
    }

    #[test]
    fn scroll_clamps_to_zero_minimum() {
        let mut app = app_with_lines(&["line1", "line2", "line3"]);
        app.scroll_extension_output(-100);
        assert_eq!(scroll(&app), 0);
    }

    #[test]
    fn scroll_clamps_to_line_count_maximum() {
        let mut app = app_with_lines(&["line1", "line2", "line3"]);
        app.scroll_extension_output(i32::MAX);
        assert_eq!(scroll(&app), 3);
    }

    #[test]
    fn scroll_works_while_running_against_lines_so_far() {
        let mut app = App::new();
        let (cancel_tx, _cancel_rx) = oneshot::channel();
        app.start_extension("Test".to_string(), cancel_tx);
        app.push_extension_line("Test", false, "line1".to_string());
        app.push_extension_line("Test", false, "line2".to_string());
        app.scroll_extension_output(100);
        assert_eq!(scroll(&app), 2);
        assert!(app.extension_running());
    }

    #[test]
    fn reopening_output_view_does_not_lose_accumulated_lines() {
        let mut app = app_with_lines(&["line1", "line2"]);
        app.close_extension_output();
        assert!(matches!(app.mode, Mode::Normal));
        app.show_extension_output();
        let run = app.extension_run.as_ref().expect("run should persist");
        assert_eq!(run.lines.len(), 2);
        assert!(!run.running);
    }

    #[test]
    fn cancelling_sends_signal_and_clears_handle() {
        let mut app = App::new();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        app.start_extension("Test".to_string(), cancel_tx);
        app.cancel_running_extension();
        assert!(cancel_rx.try_recv().is_ok());
        // Calling again is a no-op (handle already consumed), not a panic.
        app.cancel_running_extension();
    }
}
