use crate::linear::Issue;

pub enum LoadState {
    Loading,
    Loaded,
    Error(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Team,
    Project,
    Status,
}

impl FilterKind {
    pub fn is_multi(self) -> bool {
        matches!(self, FilterKind::Status)
    }
}

pub enum Mode {
    Normal,
    Filter {
        kind: FilterKind,
        options: Vec<String>,
        selected: usize,
        checked: Vec<bool>,
    },
}

pub struct App {
    pub all_issues: Vec<Issue>,
    pub issues: Vec<Issue>,
    pub selected: usize,
    pub load_state: LoadState,
    pub should_quit: bool,
    pub team_filter: Option<String>,
    pub project_filter: Option<String>,
    pub status_filter: Vec<String>,
    pub mode: Mode,
}

const ALL_LABEL: &str = "(All)";

impl App {
    pub fn new() -> Self {
        Self {
            all_issues: Vec::new(),
            issues: Vec::new(),
            selected: 0,
            load_state: LoadState::Loading,
            should_quit: false,
            team_filter: None,
            project_filter: None,
            status_filter: Vec::new(),
            mode: Mode::Normal,
        }
    }

    pub fn set_issues(&mut self, issues: Vec<Issue>) {
        self.all_issues = issues;
        self.team_filter = None;
        self.project_filter = None;
        self.status_filter.clear();
        self.load_state = LoadState::Loaded;
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
            .filter(|issue| {
                self.team_filter
                    .as_ref()
                    .map_or(true, |t| &issue.team.name == t)
            })
            .filter(|issue| {
                self.project_filter.as_ref().map_or(true, |p| {
                    issue
                        .project
                        .as_ref()
                        .map(|proj| &proj.name == p)
                        .unwrap_or(false)
                })
            })
            .filter(|issue| {
                self.status_filter.is_empty()
                    || self.status_filter.iter().any(|s| s == &issue.state.name)
            })
            .cloned()
            .collect();
        self.selected = 0;
    }

    pub fn clear_filters(&mut self) {
        self.team_filter = None;
        self.project_filter = None;
        self.status_filter.clear();
        self.apply_filters();
    }

    pub fn open_team_filter(&mut self) {
        let mut names: Vec<String> = self
            .all_issues
            .iter()
            .map(|i| i.team.name.clone())
            .collect();
        names.sort();
        names.dedup();
        let mut options = vec![ALL_LABEL.to_string()];
        options.extend(names);
        let selected = self
            .team_filter
            .as_ref()
            .and_then(|t| options.iter().position(|o| o == t))
            .unwrap_or(0);
        let checked = vec![false; options.len()];
        self.mode = Mode::Filter {
            kind: FilterKind::Team,
            options,
            selected,
            checked,
        };
    }

    pub fn open_project_filter(&mut self) {
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
            .project_filter
            .as_ref()
            .and_then(|p| options.iter().position(|o| o == p))
            .unwrap_or(0);
        let checked = vec![false; options.len()];
        self.mode = Mode::Filter {
            kind: FilterKind::Project,
            options,
            selected,
            checked,
        };
    }

    pub fn open_status_filter(&mut self) {
        let mut names: Vec<String> = self
            .all_issues
            .iter()
            .map(|i| i.state.name.clone())
            .collect();
        names.sort();
        names.dedup();
        let options = names;
        let checked: Vec<bool> = options
            .iter()
            .map(|o| self.status_filter.iter().any(|s| s == o))
            .collect();
        self.mode = Mode::Filter {
            kind: FilterKind::Status,
            options,
            selected: 0,
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
        if let Mode::Filter {
            kind,
            options,
            selected,
            checked,
        } = &self.mode
        {
            if kind.is_multi() {
                let chosen: Vec<String> = options
                    .iter()
                    .zip(checked.iter())
                    .filter(|(_, &c)| c)
                    .map(|(o, _)| o.clone())
                    .collect();
                match kind {
                    FilterKind::Status => self.status_filter = chosen,
                    _ => {}
                }
            } else {
                let choice = options.get(*selected).cloned();
                let value = choice.filter(|c| c != ALL_LABEL);
                match kind {
                    FilterKind::Team => self.team_filter = value,
                    FilterKind::Project => self.project_filter = value,
                    FilterKind::Status => {}
                }
            }
        }
        self.mode = Mode::Normal;
        self.apply_filters();
    }

    pub fn filter_cancel(&mut self) {
        self.mode = Mode::Normal;
    }
}
