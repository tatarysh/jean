use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use super::gitea_issues::{get_gitea_config, gitea_api_url, gitea_get};

// =============================================================================
// Gitea Actions Types
// =============================================================================

/// A single workflow run from `GET /repos/{owner}/{repo}/actions/tasks`.
///
/// Gitea Actions intentionally mirrors GitHub Actions' data model, so the run envelope
/// (`{workflow_runs: [...], total_count}`) matches GitHub's Actions API shape. Individual
/// run objects are parsed leniently — unknown/missing fields default rather than failing
/// the whole batch, since this schema hasn't been exhaustively verified against every
/// Gitea version.
#[derive(Debug, Clone, Deserialize)]
struct GiteaWorkflowRunRaw {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    event: String,
    #[serde(default)]
    head_branch: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    html_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GiteaWorkflowRunsEnvelope {
    #[serde(default)]
    workflow_runs: Vec<serde_json::Value>,
}

/// A single Gitea Actions workflow run
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GiteaWorkflowRun {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub event: String,
    pub head_branch: String,
    pub created_at: String,
    pub url: String,
}

impl From<GiteaWorkflowRunRaw> for GiteaWorkflowRun {
    fn from(raw: GiteaWorkflowRunRaw) -> Self {
        Self {
            id: raw.id,
            name: raw.name,
            status: raw.status,
            conclusion: raw.conclusion,
            event: raw.event,
            head_branch: raw.head_branch,
            created_at: raw.created_at,
            url: raw.html_url,
        }
    }
}

/// Result of listing workflow runs, includes failed count for badge display
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiteaWorkflowRunsResult {
    pub runs: Vec<GiteaWorkflowRun>,
    pub failed_count: u32,
}

/// List Gitea Actions workflow runs for a repository
///
/// - branch: optional branch name to filter runs (for PR/worktree-specific views)
/// - Returns up to 50 recent runs with a count of failed runs for badge display
pub async fn list_gitea_workflow_runs(
    app: AppHandle,
    project_id: String,
    branch: Option<String>,
) -> Result<GiteaWorkflowRunsResult, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let mut url = gitea_api_url(
        &config.base_url,
        &["repos", &config.owner, &config.repo, "actions", "tasks"],
    )?;
    url.query_pairs_mut().append_pair("limit", "50");
    if let Some(ref b) = branch {
        url.query_pairs_mut().append_pair("branch", b);
    }

    let value = gitea_get(&config.token, url).await?;
    let envelope: GiteaWorkflowRunsEnvelope = serde_json::from_value(value)
        .map_err(|e| format!("Unexpected Gitea actions response: {e}"))?;

    let mut runs = Vec::new();
    for raw_run in envelope.workflow_runs {
        match serde_json::from_value::<GiteaWorkflowRunRaw>(raw_run) {
            Ok(run) => runs.push(GiteaWorkflowRun::from(run)),
            Err(e) => log::debug!("Skipping unparseable Gitea workflow run: {e}"),
        }
    }

    // Count failures only for the most recent run per workflow name, matching the
    // GitHub Actions badge behavior (see github_actions.rs::list_workflow_runs).
    let mut seen_workflows = std::collections::HashSet::new();
    let mut failed_count: u32 = 0;
    for run in &runs {
        if seen_workflows.insert(&run.name)
            && matches!(
                run.conclusion.as_deref(),
                Some("failure") | Some("startup_failure")
            )
        {
            failed_count += 1;
        }
    }

    Ok(GiteaWorkflowRunsResult { runs, failed_count })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_workflow_run_envelope() {
        let json = serde_json::json!({
            "workflow_runs": [{
                "id": 6,
                "name": "CI",
                "status": "completed",
                "conclusion": "success",
                "event": "push",
                "head_branch": "master",
                "created_at": "2026-07-24T09:17:35Z",
                "html_url": "https://gitea.rysh/rysh/jean-gitea-test/actions/runs/6"
            }],
            "total_count": 1
        });
        let envelope: GiteaWorkflowRunsEnvelope = serde_json::from_value(json).unwrap();
        assert_eq!(envelope.workflow_runs.len(), 1);
        let run: GiteaWorkflowRun =
            serde_json::from_value::<GiteaWorkflowRunRaw>(envelope.workflow_runs[0].clone())
                .unwrap()
                .into();
        assert_eq!(run.id, 6);
        assert_eq!(run.name, "CI");
    }

    fn make_run(id: u64, name: &str, conclusion: Option<&str>) -> GiteaWorkflowRun {
        GiteaWorkflowRun {
            id,
            name: name.into(),
            status: "completed".into(),
            conclusion: conclusion.map(String::from),
            event: "push".into(),
            head_branch: "main".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            url: format!("https://example.com/{id}"),
        }
    }

    #[test]
    fn counts_latest_failure_per_workflow() {
        let runs = vec![
            make_run(2, "CI", Some("failure")),
            make_run(1, "CI", Some("success")),
        ];
        let mut seen = std::collections::HashSet::new();
        let mut count = 0u32;
        for run in &runs {
            if seen.insert(&run.name)
                && matches!(
                    run.conclusion.as_deref(),
                    Some("failure") | Some("startup_failure")
                )
            {
                count += 1;
            }
        }
        assert_eq!(count, 1);
    }
}
