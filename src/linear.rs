use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;

const API_URL: &str = "https://api.linear.app/graphql";

#[derive(Debug, Clone, Deserialize)]
pub struct State {
    pub name: String,
    #[serde(rename = "type")]
    pub state_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Team {
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

    pub async fn fetch_my_issues(&self) -> Result<Vec<Issue>> {
        let query = r#"
            query AllIssues {
                issues(
                    first: 150
                    orderBy: updatedAt
                ) {
                    nodes {
                        id
                        identifier
                        title
                        priority
                        state { name type }
                        team { name key }
                        project { name }
                        updatedAt
                        url
                        description
                        assignee { name }
                        inverseRelations {
                            nodes {
                                type
                                relatedIssue { state { name type } }
                            }
                        }
                    }
                }
            }
        "#;

        let resp = self
            .http
            .post(API_URL)
            .header("Authorization", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&json!({ "query": query }))
            .send()
            .await
            .context("failed to reach Linear API")?;

        let status = resp.status();
        let body: GraphQlResponse<IssuesData> = resp
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

        let data = body.data.context("Linear API response had no data")?;
        Ok(data.issues.nodes)
    }
}
