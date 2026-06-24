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
}

#[derive(Debug, Clone, Deserialize)]
pub struct Assignee {
    pub name: String,
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
