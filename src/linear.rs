use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;

const API_URL: &str = "https://api.linear.app/graphql";

#[derive(Debug, Clone, Deserialize)]
pub struct State {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub state_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Project {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Issue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub priority: f64,
    pub state: State,
    pub team: Team,
    pub project: Option<Project>,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub url: String,
    pub description: Option<String>,
    pub assignee: Option<Assignee>,
    #[serde(rename = "inverseRelations")]
    pub inverse_relations: RelationConnection,
}

impl Issue {
    /// True if another (non-completed, non-cancelled) issue blocks this one.
    pub fn is_blocked(&self) -> bool {
        self.inverse_relations
            .nodes
            .iter()
            .any(|r| r.relation_type == "blocks" && !r.related_issue.is_done())
    }

    /// Sort key where lower sorts first: Urgent(1) < High(2) < Medium(3) < Low(4) < None(0).
    pub fn priority_rank(&self) -> i64 {
        let p = self.priority as i64;
        if p == 0 {
            5
        } else {
            p
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Assignee {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RelationConnection {
    pub nodes: Vec<IssueRelation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IssueRelation {
    #[serde(rename = "type")]
    pub relation_type: String,
    #[serde(rename = "relatedIssue")]
    pub related_issue: RelatedIssue,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RelatedIssue {
    pub state: State,
}

impl RelatedIssue {
    fn is_done(&self) -> bool {
        matches!(self.state.state_type.as_str(), "completed" | "canceled")
    }
}

/// One entry in a team's workflow (its available statuses), e.g. "Todo",
/// "In Progress", "Done". Distinct from `State` (which is just the slice of
/// fields embedded on an `Issue`) since picking a new status needs the full
/// list for the issue's team, ordered the way Linear orders its own board.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowState {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub state_type: String,
    pub position: f64,
}

#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Deserialize)]
struct IssuesData {
    issues: IssueConnection,
}

#[derive(Deserialize)]
struct IssueConnection {
    nodes: Vec<Issue>,
}

#[derive(Deserialize)]
struct WorkflowStatesData {
    #[serde(rename = "workflowStates")]
    workflow_states: WorkflowStateConnection,
}

#[derive(Deserialize)]
struct WorkflowStateConnection {
    nodes: Vec<WorkflowState>,
}

#[derive(Deserialize)]
struct IssueUpdateData {
    #[serde(rename = "issueUpdate")]
    issue_update: MutationResult,
}

#[derive(Deserialize)]
struct IssueCreateData {
    #[serde(rename = "issueCreate")]
    issue_create: IssueCreateResult,
}

#[derive(Deserialize)]
struct IssueCreateResult {
    success: bool,
    issue: Option<Issue>,
}

#[derive(Deserialize)]
struct MutationResult {
    success: bool,
}

#[derive(Deserialize)]
struct TeamsData {
    teams: TeamConnection,
}

#[derive(Deserialize)]
struct TeamConnection {
    nodes: Vec<TeamWithProjects>,
}

#[derive(Deserialize)]
struct TeamWithProjects {
    id: String,
    projects: ProjectConnection,
}

#[derive(Deserialize)]
struct ProjectConnection {
    nodes: Vec<ProjectWithId>,
}

#[derive(Deserialize)]
struct ProjectWithId {
    id: String,
    name: String,
}

/// Fields shared by every query/mutation that returns a full issue, so the
/// list and the create-issue response can't drift out of sync.
const ISSUE_FIELDS: &str = r#"
    id
    identifier
    title
    priority
    state { id name type }
    team { id name key }
    project { name }
    updatedAt
    url
    description
    assignee { name }
    inverseRelations {
        nodes {
            type
            relatedIssue { state { id name type } }
        }
    }
"#;

pub struct Client {
    api_key: String,
    http: reqwest::Client,
}

impl Client {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http: reqwest::Client::new(),
        }
    }

    async fn request<T: for<'de> Deserialize<'de>>(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<T> {
        let resp = self
            .http
            .post(API_URL)
            .header("Authorization", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .context("failed to reach Linear API")?;

        let status = resp.status();
        let body: GraphQlResponse<T> = resp
            .json()
            .await
            .context("failed to parse Linear API response")?;

        if let Some(errors) = body.errors {
            let msgs: Vec<String> = errors.into_iter().map(|e| e.message).collect();
            bail!("Linear API error(s): {}", msgs.join("; "));
        }

        if !status.is_success() {
            bail!("Linear API request failed with status {}", status);
        }

        body.data.context("Linear API response had no data")
    }

    pub async fn fetch_my_issues(&self) -> Result<Vec<Issue>> {
        let query = format!(
            r#"
            query AllIssues {{
                issues(
                    first: 150
                    orderBy: updatedAt
                ) {{
                    nodes {{ {ISSUE_FIELDS} }}
                }}
            }}
        "#
        );

        let data: IssuesData = self.request(&query, json!({})).await?;
        Ok(data.issues.nodes)
    }

    /// All workflow states (statuses) configured for the given team, ordered
    /// the way Linear orders them on its own board, for presenting a
    /// "change status" picker.
    pub async fn fetch_workflow_states(&self, team_name: &str) -> Result<Vec<WorkflowState>> {
        let query = r#"
            query TeamStates($name: String!) {
                workflowStates(filter: { team: { name: { eq: $name } } }) {
                    nodes { id name type position }
                }
            }
        "#;

        let data: WorkflowStatesData = self
            .request(query, json!({ "name": team_name }))
            .await?;
        let mut states = data.workflow_states.nodes;
        states.sort_by(|a, b| a.position.partial_cmp(&b.position).unwrap());
        Ok(states)
    }

    /// Moves an issue to a new status.
    pub async fn update_issue_state(&self, issue_id: &str, state_id: &str) -> Result<()> {
        let query = r#"
            mutation UpdateState($id: String!, $stateId: String!) {
                issueUpdate(id: $id, input: { stateId: $stateId }) {
                    success
                }
            }
        "#;

        let data: IssueUpdateData = self
            .request(query, json!({ "id": issue_id, "stateId": state_id }))
            .await?;
        if !data.issue_update.success {
            bail!("Linear API reported failure updating issue status");
        }
        Ok(())
    }

    /// Resolves a team name (and, if given, a project name within it) to the
    /// ids Linear's mutations need, since the rest of harness only knows
    /// teams/projects by name (from `projects.toml` and issue data).
    pub async fn resolve_team_project_ids(
        &self,
        team_name: &str,
        project_name: Option<&str>,
    ) -> Result<(String, Option<String>)> {
        let query = r#"
            query TeamProject($name: String!) {
                teams(filter: { name: { eq: $name } }) {
                    nodes {
                        id
                        projects { nodes { id name } }
                    }
                }
            }
        "#;

        let data: TeamsData = self
            .request(query, json!({ "name": team_name }))
            .await?;
        let team = data
            .teams
            .nodes
            .into_iter()
            .next()
            .with_context(|| format!("no Linear team named '{team_name}'"))?;

        let project_id = match project_name {
            Some(pname) => {
                let project = team
                    .projects
                    .nodes
                    .into_iter()
                    .find(|p| p.name == pname)
                    .with_context(|| {
                        format!("no project named '{pname}' in team '{team_name}'")
                    })?;
                Some(project.id)
            }
            None => None,
        };

        Ok((team.id, project_id))
    }

    /// Creates a new issue against the given team (and optional project),
    /// returning it in the same shape as `fetch_my_issues` so it can be
    /// dropped straight into the app's issue list.
    pub async fn create_issue(
        &self,
        team_id: &str,
        project_id: Option<&str>,
        title: &str,
    ) -> Result<Issue> {
        let query = format!(
            r#"
            mutation CreateIssue($teamId: String!, $projectId: String, $title: String!) {{
                issueCreate(input: {{ teamId: $teamId, projectId: $projectId, title: $title }}) {{
                    success
                    issue {{ {ISSUE_FIELDS} }}
                }}
            }}
        "#
        );

        let data: IssueCreateData = self
            .request(
                &query,
                json!({ "teamId": team_id, "projectId": project_id, "title": title }),
            )
            .await?;
        if !data.issue_create.success {
            bail!("Linear API reported failure creating issue");
        }
        data.issue_create
            .issue
            .context("Linear API did not return the created issue")
    }
}
