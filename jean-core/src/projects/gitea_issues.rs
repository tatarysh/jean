use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

use super::github_issues::{
    get_github_contexts_dir, load_context_references, save_context_references, slugify_issue_title,
};
use super::storage::load_projects_data;

// =============================================================================
// Config resolution — per-project URL, token, owner, repo (see IntegrationsPane.tsx)
// =============================================================================

pub(crate) struct GiteaConfig {
    pub(crate) base_url: String,
    pub(crate) token: String,
    pub(crate) owner: String,
    pub(crate) repo: String,
}

pub(crate) fn get_gitea_config(app: &AppHandle, project_id: &str) -> Result<GiteaConfig, String> {
    let data = load_projects_data(app)?;
    let project = data
        .find_project(project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?;

    let base_url = project
        .gitea_url
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "No Gitea instance URL configured. Add it in project settings.".to_string()
        })?;
    let token = project
        .gitea_token
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "No Gitea access token configured. Add it in project settings.".to_string()
        })?;
    let owner = project
        .gitea_owner
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "No Gitea repository owner configured. Add it in project settings.".to_string()
        })?;
    let repo = project
        .gitea_repo
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "No Gitea repository name configured. Add it in project settings.".to_string()
        })?;

    Ok(GiteaConfig {
        base_url,
        token,
        owner,
        repo,
    })
}

/// Repo key used for shared context file naming: "{owner}-{repo}"
fn gitea_repo_key(config: &GiteaConfig) -> String {
    format!("{}-{}", config.owner, config.repo)
}

// =============================================================================
// HTTP client
// =============================================================================

pub(crate) fn gitea_api_url(base_url: &str, segments: &[&str]) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url).map_err(|e| format!("Invalid Gitea URL: {e}"))?;
    let mut path = url
        .path_segments_mut()
        .map_err(|_| "Invalid Gitea URL".to_string())?;
    path.clear().extend(["api", "v1"]).extend(segments);
    drop(path);
    Ok(url)
}

pub(crate) async fn gitea_get(token: &str, url: reqwest::Url) -> Result<serde_json::Value, String> {
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("token {token}"))
        .send()
        .await
        .map_err(|e| format!("Gitea API request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(
                "Gitea access token is invalid or missing required permissions. Update it in project settings."
                    .to_string(),
            );
        }
        if status.as_u16() == 404 {
            return Err(
                "Gitea repository was not found. Check the owner/repo and instance URL in project settings."
                    .to_string(),
            );
        }
        return Err(format!("Gitea API error ({status}): {text}"));
    }

    response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Gitea response: {e}"))
}

/// Best-effort fetch of a raw (non-JSON) Gitea URL, e.g. a PR diff. Returns `None` on any
/// failure rather than erroring, mirroring `get_pr_diff`'s tolerant behavior on the GitHub side.
async fn gitea_get_text_lenient(token: &str, url: &str) -> Option<String> {
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("token {token}"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let text = response.text().await.ok()?;
    // The `.diff` route can fall back to an HTML page (e.g. login redirect) on instances
    // that require authentication for anonymous web access; only trust plain-text bodies.
    if text.trim_start().starts_with("<!DOCTYPE") || text.trim_start().starts_with("<html") {
        return None;
    }
    Some(text)
}

/// Result of a connection test, shown in the "Test Connection" button in
/// IntegrationsPane.tsx.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiteaConnectionStatus {
    pub full_name: String,
    pub html_url: String,
}

/// Verify that the project's Gitea URL, token, owner, and repo are all valid by
/// fetching the repository itself. Surfaces the same config/auth errors as any
/// other Gitea call, so the UI can reuse `isGiteaConfigError`/`isGiteaAuthError`.
pub async fn test_gitea_connection(
    app: AppHandle,
    project_id: String,
) -> Result<GiteaConnectionStatus, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let url = gitea_api_url(&config.base_url, &["repos", &config.owner, &config.repo])?;
    let value = gitea_get(&config.token, url).await?;
    let full_name = value
        .get("full_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let html_url = value
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok(GiteaConnectionStatus {
        full_name,
        html_url,
    })
}

// =============================================================================
// Types — raw Gitea API shapes (deserialize-only) and frontend-facing types
// =============================================================================

#[derive(Debug, Clone, Deserialize)]
struct GiteaUserRaw {
    login: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GiteaLabelRaw {
    name: String,
    color: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GiteaIssueRaw {
    number: u32,
    title: String,
    body: Option<String>,
    state: String,
    #[serde(default)]
    labels: Vec<GiteaLabelRaw>,
    created_at: String,
    user: GiteaUserRaw,
    #[serde(default)]
    html_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GiteaCommentRaw {
    body: String,
    user: GiteaUserRaw,
    created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GiteaPrBranchRaw {
    #[serde(rename = "ref")]
    ref_name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GiteaPullRequestRaw {
    number: u32,
    title: String,
    body: Option<String>,
    state: String,
    head: GiteaPrBranchRaw,
    base: GiteaPrBranchRaw,
    #[serde(default)]
    draft: bool,
    created_at: String,
    user: GiteaUserRaw,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    labels: Vec<GiteaLabelRaw>,
}

#[derive(Debug, Clone, Deserialize)]
struct GiteaReviewRaw {
    id: u64,
    #[serde(default)]
    body: String,
    state: String,
    user: GiteaUserRaw,
    submitted_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GiteaReviewCommentRaw {
    body: String,
    user: GiteaUserRaw,
    created_at: String,
    #[serde(default)]
    diff_hunk: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    line: Option<u32>,
}

/// Gitea issue/PR label
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GiteaLabel {
    pub name: String,
    pub color: String,
}

impl From<GiteaLabelRaw> for GiteaLabel {
    fn from(raw: GiteaLabelRaw) -> Self {
        Self {
            name: raw.name,
            color: raw.color,
        }
    }
}

/// Gitea user/author
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GiteaAuthor {
    pub login: String,
}

impl From<GiteaUserRaw> for GiteaAuthor {
    fn from(raw: GiteaUserRaw) -> Self {
        Self { login: raw.login }
    }
}

/// Gitea issue from list response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GiteaIssue {
    pub number: u32,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub labels: Vec<GiteaLabel>,
    pub created_at: String,
    pub author: GiteaAuthor,
}

impl From<GiteaIssueRaw> for GiteaIssue {
    fn from(raw: GiteaIssueRaw) -> Self {
        Self {
            number: raw.number,
            title: raw.title,
            body: raw.body,
            state: raw.state,
            labels: raw.labels.into_iter().map(Into::into).collect(),
            created_at: raw.created_at,
            author: raw.user.into(),
        }
    }
}

/// Gitea comment
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GiteaComment {
    pub body: String,
    pub author: GiteaAuthor,
    pub created_at: String,
}

impl From<GiteaCommentRaw> for GiteaComment {
    fn from(raw: GiteaCommentRaw) -> Self {
        Self {
            body: raw.body,
            author: raw.user.into(),
            created_at: raw.created_at,
        }
    }
}

/// Gitea issue detail with comments
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GiteaIssueDetail {
    pub number: u32,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub labels: Vec<GiteaLabel>,
    pub created_at: String,
    pub author: GiteaAuthor,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub comments: Vec<GiteaComment>,
}

/// Issue context to pass when creating a worktree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GiteaIssueContext {
    pub number: u32,
    pub title: String,
    pub body: Option<String>,
    pub comments: Vec<GiteaComment>,
}

/// Loaded issue context info returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedGiteaIssueContext {
    pub number: u32,
    pub title: String,
    pub comment_count: usize,
    pub repo_owner: String,
    pub repo_name: String,
}

/// Gitea pull request from list response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GiteaPullRequest {
    pub number: u32,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub is_draft: bool,
    pub created_at: String,
    pub author: GiteaAuthor,
    #[serde(default)]
    pub labels: Vec<GiteaLabel>,
}

impl From<GiteaPullRequestRaw> for GiteaPullRequest {
    fn from(raw: GiteaPullRequestRaw) -> Self {
        Self {
            number: raw.number,
            title: raw.title,
            body: raw.body,
            state: raw.state,
            head_ref_name: raw.head.ref_name,
            base_ref_name: raw.base.ref_name,
            is_draft: raw.draft,
            created_at: raw.created_at,
            author: raw.user.into(),
            labels: raw.labels.into_iter().map(Into::into).collect(),
        }
    }
}

/// Gitea PR review
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GiteaReview {
    pub body: String,
    pub state: String,
    pub author: GiteaAuthor,
    pub submitted_at: Option<String>,
}

impl From<GiteaReviewRaw> for GiteaReview {
    fn from(raw: GiteaReviewRaw) -> Self {
        Self {
            body: raw.body,
            state: raw.state,
            author: raw.user.into(),
            submitted_at: raw.submitted_at,
        }
    }
}

/// Gitea inline review comment (on specific diff lines)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GiteaReviewComment {
    pub author: GiteaAuthor,
    pub body: String,
    pub created_at: String,
    pub diff_hunk: String,
    pub path: String,
    #[serde(default)]
    pub line: Option<u32>,
}

impl From<GiteaReviewCommentRaw> for GiteaReviewComment {
    fn from(raw: GiteaReviewCommentRaw) -> Self {
        Self {
            author: raw.user.into(),
            body: raw.body,
            created_at: raw.created_at,
            diff_hunk: raw.diff_hunk,
            path: raw.path,
            line: raw.line,
        }
    }
}

/// Gitea PR detail with comments and reviews
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GiteaPullRequestDetail {
    pub number: u32,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub is_draft: bool,
    pub created_at: String,
    pub author: GiteaAuthor,
    #[serde(default)]
    pub labels: Vec<GiteaLabel>,
    #[serde(default)]
    pub comments: Vec<GiteaComment>,
    #[serde(default)]
    pub reviews: Vec<GiteaReview>,
}

/// PR context to pass when creating a worktree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GiteaPullRequestContext {
    pub number: u32,
    pub title: String,
    pub body: Option<String>,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub comments: Vec<GiteaComment>,
    pub reviews: Vec<GiteaReview>,
    pub diff: Option<String>,
}

/// Loaded PR context info returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedGiteaPullRequestContext {
    pub number: u32,
    pub title: String,
    pub comment_count: usize,
    pub review_count: usize,
    pub repo_owner: String,
    pub repo_name: String,
}

// =============================================================================
// Issue commands
// =============================================================================

pub async fn list_gitea_issues(
    app: AppHandle,
    project_id: String,
    state: Option<String>,
) -> Result<Vec<GiteaIssue>, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let state = state.unwrap_or_else(|| "open".to_string());
    let mut url = gitea_api_url(
        &config.base_url,
        &["repos", &config.owner, &config.repo, "issues"],
    )?;
    url.query_pairs_mut()
        .append_pair("type", "issues")
        .append_pair("state", &state)
        .append_pair("limit", "50");

    let value = gitea_get(&config.token, url).await?;
    let raw: Vec<GiteaIssueRaw> = serde_json::from_value(value)
        .map_err(|e| format!("Unexpected Gitea issues response: {e}"))?;
    Ok(raw.into_iter().map(Into::into).collect())
}

/// Search issues by keyword. Gitea's list endpoint supports a `q` filter, so this reuses
/// the same request as `list_gitea_issues` rather than a separate search endpoint.
pub async fn search_gitea_issues(
    app: AppHandle,
    project_id: String,
    query: String,
) -> Result<Vec<GiteaIssue>, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let mut url = gitea_api_url(
        &config.base_url,
        &["repos", &config.owner, &config.repo, "issues"],
    )?;
    url.query_pairs_mut()
        .append_pair("type", "issues")
        .append_pair("state", "all")
        .append_pair("q", &query)
        .append_pair("limit", "50");

    let value = gitea_get(&config.token, url).await?;
    let raw: Vec<GiteaIssueRaw> = serde_json::from_value(value)
        .map_err(|e| format!("Unexpected Gitea issues response: {e}"))?;
    Ok(raw.into_iter().map(Into::into).collect())
}

pub async fn get_gitea_issue_by_number(
    app: AppHandle,
    project_id: String,
    issue_number: u32,
) -> Result<GiteaIssue, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let url = gitea_api_url(
        &config.base_url,
        &[
            "repos",
            &config.owner,
            &config.repo,
            "issues",
            &issue_number.to_string(),
        ],
    )?;
    let value = gitea_get(&config.token, url).await?;
    let raw: GiteaIssueRaw = serde_json::from_value(value)
        .map_err(|e| format!("Unexpected Gitea issue response: {e}"))?;
    Ok(raw.into())
}

pub async fn get_gitea_issue(
    app: AppHandle,
    project_id: String,
    issue_number: u32,
) -> Result<GiteaIssueDetail, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let issue_url = gitea_api_url(
        &config.base_url,
        &[
            "repos",
            &config.owner,
            &config.repo,
            "issues",
            &issue_number.to_string(),
        ],
    )?;
    let issue_value = gitea_get(&config.token, issue_url).await?;
    let issue: GiteaIssueRaw = serde_json::from_value(issue_value)
        .map_err(|e| format!("Unexpected Gitea issue response: {e}"))?;

    let comments_url = gitea_api_url(
        &config.base_url,
        &[
            "repos",
            &config.owner,
            &config.repo,
            "issues",
            &issue_number.to_string(),
            "comments",
        ],
    )?;
    let comments_value = gitea_get(&config.token, comments_url).await?;
    let comments: Vec<GiteaCommentRaw> = serde_json::from_value(comments_value)
        .map_err(|e| format!("Unexpected Gitea comments response: {e}"))?;

    Ok(GiteaIssueDetail {
        number: issue.number,
        title: issue.title,
        body: issue.body,
        state: issue.state,
        labels: issue.labels.into_iter().map(Into::into).collect(),
        created_at: issue.created_at,
        author: issue.user.into(),
        url: issue.html_url,
        comments: comments.into_iter().map(Into::into).collect(),
    })
}

pub fn generate_branch_name_from_gitea_issue(issue_number: u32, title: &str) -> String {
    format!("issue-{issue_number}-{}", slugify_issue_title(title))
}

fn format_gitea_issue_context_markdown(ctx: &GiteaIssueContext) -> String {
    let mut content = String::new();
    content.push_str(&format!("# Gitea Issue #{}: {}\n\n", ctx.number, ctx.title));
    content.push_str("---\n\n");
    content.push_str("## Description\n\n");
    match &ctx.body {
        Some(body) if !body.is_empty() => content.push_str(body),
        _ => content.push_str("*No description provided.*"),
    }
    content.push_str("\n\n");

    if !ctx.comments.is_empty() {
        content.push_str("## Comments\n\n");
        for comment in &ctx.comments {
            content.push_str(&format!(
                "### @{} ({})\n\n",
                comment.author.login, comment.created_at
            ));
            content.push_str(&comment.body);
            content.push_str("\n\n---\n\n");
        }
    }

    content.push_str("---\n\n");
    content.push_str("*Investigate this issue and propose a solution.*\n");
    content
}

// =============================================================================
// Gitea-specific context reference tracking
//
// Uses the shared `git-context/references.json` (see github_issues.rs), but under
// dedicated `gitea_issues`/`gitea_prs` maps and `gitea-` prefixed filenames so a
// GitHub repo and a Gitea repo that happen to share the same owner/repo name never
// collide.
// =============================================================================

fn add_gitea_issue_reference(
    app: &AppHandle,
    repo_key: &str,
    issue_number: u32,
    session_id: &str,
) -> Result<(), String> {
    let mut refs = load_context_references(app)?;
    let key = format!("{repo_key}-{issue_number}");
    let entry = refs.gitea_issues.entry(key).or_default();
    if !entry.sessions.contains(&session_id.to_string()) {
        entry.sessions.push(session_id.to_string());
    }
    entry.orphaned_at = None;
    save_context_references(app, &refs)
}

fn remove_gitea_issue_reference(
    app: &AppHandle,
    repo_key: &str,
    issue_number: u32,
    session_id: &str,
) -> Result<bool, String> {
    let mut refs = load_context_references(app)?;
    let key = format!("{repo_key}-{issue_number}");

    let orphaned = if let Some(entry) = refs.gitea_issues.get_mut(&key) {
        entry.sessions.retain(|s| s != session_id);
        if entry.sessions.is_empty() && entry.orphaned_at.is_none() {
            entry.orphaned_at = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            );
            true
        } else {
            false
        }
    } else {
        false
    };

    save_context_references(app, &refs)?;
    Ok(orphaned)
}

fn get_session_gitea_issue_refs(app: &AppHandle, session_id: &str) -> Result<Vec<String>, String> {
    let refs = load_context_references(app)?;
    Ok(refs
        .gitea_issues
        .iter()
        .filter(|(_, entry)| entry.sessions.contains(&session_id.to_string()))
        .map(|(key, _)| key.clone())
        .collect())
}

fn add_gitea_pr_reference(
    app: &AppHandle,
    repo_key: &str,
    pr_number: u32,
    session_id: &str,
) -> Result<(), String> {
    let mut refs = load_context_references(app)?;
    let key = format!("{repo_key}-{pr_number}");
    let entry = refs.gitea_prs.entry(key).or_default();
    if !entry.sessions.contains(&session_id.to_string()) {
        entry.sessions.push(session_id.to_string());
    }
    entry.orphaned_at = None;
    save_context_references(app, &refs)
}

fn remove_gitea_pr_reference(
    app: &AppHandle,
    repo_key: &str,
    pr_number: u32,
    session_id: &str,
) -> Result<bool, String> {
    let mut refs = load_context_references(app)?;
    let key = format!("{repo_key}-{pr_number}");

    let orphaned = if let Some(entry) = refs.gitea_prs.get_mut(&key) {
        entry.sessions.retain(|s| s != session_id);
        if entry.sessions.is_empty() && entry.orphaned_at.is_none() {
            entry.orphaned_at = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            );
            true
        } else {
            false
        }
    } else {
        false
    };

    save_context_references(app, &refs)?;
    Ok(orphaned)
}

fn get_session_gitea_pr_refs(app: &AppHandle, session_id: &str) -> Result<Vec<String>, String> {
    let refs = load_context_references(app)?;
    Ok(refs
        .gitea_prs
        .iter()
        .filter(|(_, entry)| entry.sessions.contains(&session_id.to_string()))
        .map(|(key, _)| key.clone())
        .collect())
}

/// Parse a context key into (repo_owner, repo_name, number). Key format: "{owner}-{repo}-{number}"
fn parse_gitea_context_key(key: &str) -> Option<(String, String, u32)> {
    let (repo_key, number_str) = key.rsplit_once('-')?;
    let number = number_str.parse::<u32>().ok()?;
    let (owner, repo) = repo_key.split_once('-')?;
    Some((owner.to_string(), repo.to_string(), number))
}

pub async fn load_gitea_issue_context(
    app: AppHandle,
    session_id: String,
    issue_number: u32,
    project_id: String,
) -> Result<LoadedGiteaIssueContext, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let repo_key = gitea_repo_key(&config);

    let issue = get_gitea_issue(app.clone(), project_id, issue_number).await?;

    let ctx = GiteaIssueContext {
        number: issue.number,
        title: issue.title.clone(),
        body: issue.body,
        comments: issue.comments,
    };

    let contexts_dir = get_github_contexts_dir(&app)?;
    std::fs::create_dir_all(&contexts_dir)
        .map_err(|e| format!("Failed to create git-context directory: {e}"))?;

    let context_file = contexts_dir.join(format!("gitea-{repo_key}-issue-{issue_number}.md"));
    std::fs::write(&context_file, format_gitea_issue_context_markdown(&ctx))
        .map_err(|e| format!("Failed to write issue context file: {e}"))?;

    add_gitea_issue_reference(&app, &repo_key, issue_number, &session_id)?;

    Ok(LoadedGiteaIssueContext {
        number: issue.number,
        title: issue.title,
        comment_count: ctx.comments.len(),
        repo_owner: config.owner,
        repo_name: config.repo,
    })
}

pub async fn list_loaded_gitea_issue_contexts(
    app: AppHandle,
    session_id: String,
    worktree_id: Option<String>,
) -> Result<Vec<LoadedGiteaIssueContext>, String> {
    let mut issue_keys = get_session_gitea_issue_refs(&app, &session_id)?;
    if let Some(ref wt_id) = worktree_id {
        if let Ok(wt_keys) = get_session_gitea_issue_refs(&app, wt_id) {
            for key in wt_keys {
                if !issue_keys.contains(&key) {
                    issue_keys.push(key);
                }
            }
        }
    }

    if issue_keys.is_empty() {
        return Ok(vec![]);
    }

    let contexts_dir = get_github_contexts_dir(&app)?;
    let mut contexts = Vec::new();

    for key in issue_keys {
        if let Some((owner, repo, number)) = parse_gitea_context_key(&key) {
            let repo_key = format!("{owner}-{repo}");
            let context_file = contexts_dir.join(format!("gitea-{repo_key}-issue-{number}.md"));

            if let Ok(content) = std::fs::read_to_string(&context_file) {
                let title = content
                    .lines()
                    .next()
                    .and_then(|line| {
                        line.strip_prefix("# Gitea Issue #")
                            .and_then(|rest| rest.split_once(": "))
                            .map(|(_, title)| title.to_string())
                    })
                    .unwrap_or_else(|| format!("Issue #{number}"));

                let comment_count = content.matches("### @").count();

                contexts.push(LoadedGiteaIssueContext {
                    number,
                    title,
                    comment_count,
                    repo_owner: owner,
                    repo_name: repo,
                });
            }
        }
    }

    contexts.sort_by_key(|c| c.number);
    Ok(contexts)
}

pub async fn remove_gitea_issue_context(
    app: AppHandle,
    session_id: String,
    issue_number: u32,
    project_id: String,
) -> Result<(), String> {
    let config = get_gitea_config(&app, &project_id)?;
    let repo_key = gitea_repo_key(&config);

    let is_orphaned = remove_gitea_issue_reference(&app, &repo_key, issue_number, &session_id)?;

    if is_orphaned {
        let contexts_dir = get_github_contexts_dir(&app)?;
        let context_file = contexts_dir.join(format!("gitea-{repo_key}-issue-{issue_number}.md"));
        if context_file.exists() {
            std::fs::remove_file(&context_file)
                .map_err(|e| format!("Failed to remove issue context file: {e}"))?;
        }
    }

    Ok(())
}

pub async fn get_gitea_issue_context_content(
    app: AppHandle,
    session_id: String,
    issue_number: u32,
    project_id: String,
) -> Result<String, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let repo_key = gitea_repo_key(&config);

    let refs = get_session_gitea_issue_refs(&app, &session_id)?;
    let expected_key = format!("{repo_key}-{issue_number}");
    if !refs.contains(&expected_key) {
        return Err(format!(
            "Session does not have issue #{issue_number} loaded"
        ));
    }

    let contexts_dir = get_github_contexts_dir(&app)?;
    let context_file = contexts_dir.join(format!("gitea-{repo_key}-issue-{issue_number}.md"));
    if !context_file.exists() {
        return Err(format!(
            "Issue context file not found for issue #{issue_number}"
        ));
    }
    std::fs::read_to_string(&context_file)
        .map_err(|e| format!("Failed to read issue context file: {e}"))
}

// =============================================================================
// Pull request commands
// =============================================================================

pub async fn list_gitea_prs(
    app: AppHandle,
    project_id: String,
    state: Option<String>,
) -> Result<Vec<GiteaPullRequest>, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let state = state.unwrap_or_else(|| "open".to_string());
    let mut url = gitea_api_url(
        &config.base_url,
        &["repos", &config.owner, &config.repo, "pulls"],
    )?;
    url.query_pairs_mut()
        .append_pair("state", &state)
        .append_pair("limit", "50");

    let value = gitea_get(&config.token, url).await?;
    let raw: Vec<GiteaPullRequestRaw> =
        serde_json::from_value(value).map_err(|e| format!("Unexpected Gitea PR response: {e}"))?;
    Ok(raw.into_iter().map(Into::into).collect())
}

/// Search PRs by keyword. Gitea's `/pulls` endpoint has no server-side search filter,
/// so this fetches the (already capped) list and filters client-side.
pub async fn search_gitea_prs(
    app: AppHandle,
    project_id: String,
    query: String,
) -> Result<Vec<GiteaPullRequest>, String> {
    let prs = list_gitea_prs(app, project_id, Some("all".to_string())).await?;
    let lower = query.to_lowercase();
    Ok(prs
        .into_iter()
        .filter(|pr| {
            pr.title.to_lowercase().contains(&lower)
                || pr.number.to_string().contains(&lower)
                || pr
                    .body
                    .as_ref()
                    .is_some_and(|body| body.to_lowercase().contains(&lower))
        })
        .collect())
}

pub async fn get_gitea_pr_by_number(
    app: AppHandle,
    project_id: String,
    pr_number: u32,
) -> Result<GiteaPullRequest, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let url = gitea_api_url(
        &config.base_url,
        &[
            "repos",
            &config.owner,
            &config.repo,
            "pulls",
            &pr_number.to_string(),
        ],
    )?;
    let value = gitea_get(&config.token, url).await?;
    let raw: GiteaPullRequestRaw =
        serde_json::from_value(value).map_err(|e| format!("Unexpected Gitea PR response: {e}"))?;
    Ok(raw.into())
}

pub async fn get_gitea_pr(
    app: AppHandle,
    project_id: String,
    pr_number: u32,
) -> Result<GiteaPullRequestDetail, String> {
    let config = get_gitea_config(&app, &project_id)?;

    let pr_url = gitea_api_url(
        &config.base_url,
        &[
            "repos",
            &config.owner,
            &config.repo,
            "pulls",
            &pr_number.to_string(),
        ],
    )?;
    let pr_value = gitea_get(&config.token, pr_url).await?;
    let pr: GiteaPullRequestRaw = serde_json::from_value(pr_value)
        .map_err(|e| format!("Unexpected Gitea PR response: {e}"))?;

    // PR conversation comments live under the issues endpoint, matching Gitea's data model
    // where every PR is also an issue.
    let comments_url = gitea_api_url(
        &config.base_url,
        &[
            "repos",
            &config.owner,
            &config.repo,
            "issues",
            &pr_number.to_string(),
            "comments",
        ],
    )?;
    let comments: Vec<GiteaCommentRaw> = match gitea_get(&config.token, comments_url).await {
        Ok(value) => serde_json::from_value(value).unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let reviews_url = gitea_api_url(
        &config.base_url,
        &[
            "repos",
            &config.owner,
            &config.repo,
            "pulls",
            &pr_number.to_string(),
            "reviews",
        ],
    )?;
    let reviews: Vec<GiteaReviewRaw> = match gitea_get(&config.token, reviews_url).await {
        Ok(value) => serde_json::from_value(value).unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    Ok(GiteaPullRequestDetail {
        number: pr.number,
        title: pr.title,
        body: pr.body,
        state: pr.state,
        head_ref_name: pr.head.ref_name,
        base_ref_name: pr.base.ref_name,
        is_draft: pr.draft,
        created_at: pr.created_at,
        author: pr.user.into(),
        labels: pr.labels.into_iter().map(Into::into).collect(),
        comments: comments.into_iter().map(Into::into).collect(),
        reviews: reviews.into_iter().map(Into::into).collect(),
    })
}

/// Fetch inline review comments for a PR. Gitea has no single "all inline comments"
/// endpoint like GitHub's GraphQL review threads, so this lists reviews first and then
/// fetches each review's comments, aggregating the results. Unlike the GitHub side there
/// is no "outdated" concept to filter — all inline comments are returned.
pub async fn get_gitea_pr_review_comments(
    app: AppHandle,
    project_id: String,
    pr_number: u32,
) -> Result<Vec<GiteaReviewComment>, String> {
    let config = get_gitea_config(&app, &project_id)?;

    let reviews_url = gitea_api_url(
        &config.base_url,
        &[
            "repos",
            &config.owner,
            &config.repo,
            "pulls",
            &pr_number.to_string(),
            "reviews",
        ],
    )?;
    let reviews_value = gitea_get(&config.token, reviews_url).await?;
    let reviews: Vec<GiteaReviewRaw> = serde_json::from_value(reviews_value)
        .map_err(|e| format!("Unexpected Gitea reviews response: {e}"))?;

    let mut comments = Vec::new();
    for review in reviews {
        let comments_url = gitea_api_url(
            &config.base_url,
            &[
                "repos",
                &config.owner,
                &config.repo,
                "pulls",
                &pr_number.to_string(),
                "reviews",
                &review.id.to_string(),
                "comments",
            ],
        );
        let Ok(comments_url) = comments_url else {
            continue;
        };
        match gitea_get(&config.token, comments_url).await {
            Ok(value) => {
                if let Ok(raw) = serde_json::from_value::<Vec<GiteaReviewCommentRaw>>(value) {
                    comments.extend(raw.into_iter().map(Into::into));
                }
            }
            Err(e) => {
                log::debug!(
                    "Failed to fetch comments for Gitea review {}: {e}",
                    review.id
                );
            }
        }
    }

    Ok(comments)
}

/// Fetch the diff for a PR. Returns `None` if the diff route is unavailable (e.g. the
/// instance requires an authenticated web session rather than token auth for raw routes),
/// mirroring the GitHub side's tolerant `get_pr_diff` behavior.
pub async fn get_gitea_pr_diff(
    app: AppHandle,
    project_id: String,
    pr_number: u32,
) -> Result<Option<String>, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let diff_url = format!(
        "{}/{}/{}/pulls/{}.diff",
        config.base_url.trim_end_matches('/'),
        config.owner,
        config.repo,
        pr_number
    );

    const MAX_DIFF_SIZE: usize = 100_000;
    Ok(gitea_get_text_lenient(&config.token, &diff_url)
        .await
        .map(|diff| {
            if diff.len() > MAX_DIFF_SIZE {
                let end = diff
                    .char_indices()
                    .take_while(|(i, _)| *i < MAX_DIFF_SIZE)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(MAX_DIFF_SIZE.min(diff.len()));
                format!(
                    "{}...\n\n[Diff truncated at 100KB - {} bytes total.]",
                    &diff[..end],
                    diff.len()
                )
            } else {
                diff
            }
        }))
}

pub fn generate_branch_name_from_gitea_pr(pr_number: u32, title: &str) -> String {
    format!("pr-{pr_number}-{}", slugify_issue_title(title))
}

fn format_gitea_pr_context_markdown(ctx: &GiteaPullRequestContext) -> String {
    let mut content = String::new();
    content.push_str(&format!(
        "# Gitea Pull Request #{}: {}\n\n",
        ctx.number, ctx.title
    ));
    content.push_str(&format!(
        "**Branch:** `{}` \u{2192} `{}`\n\n",
        ctx.head_ref_name, ctx.base_ref_name
    ));
    content.push_str("---\n\n");
    content.push_str("## Description\n\n");
    match &ctx.body {
        Some(body) if !body.is_empty() => content.push_str(body),
        _ => content.push_str("*No description provided.*"),
    }
    content.push_str("\n\n");

    if !ctx.reviews.is_empty() {
        content.push_str("## Reviews\n\n");
        for review in &ctx.reviews {
            let submitted = review.submitted_at.as_deref().unwrap_or("Unknown date");
            content.push_str(&format!(
                "### @{} - {} ({})\n\n",
                review.author.login, review.state, submitted
            ));
            if !review.body.is_empty() {
                content.push_str(&review.body);
                content.push_str("\n\n");
            }
            content.push_str("---\n\n");
        }
    }

    if !ctx.comments.is_empty() {
        content.push_str("## Comments\n\n");
        for comment in &ctx.comments {
            content.push_str(&format!(
                "### @{} ({})\n\n",
                comment.author.login, comment.created_at
            ));
            content.push_str(&comment.body);
            content.push_str("\n\n---\n\n");
        }
    }

    if let Some(diff) = &ctx.diff {
        if !diff.is_empty() {
            content.push_str("## Changes (Diff)\n\n");
            content.push_str("```diff\n");
            content.push_str(diff);
            if !diff.ends_with('\n') {
                content.push('\n');
            }
            content.push_str("```\n\n");
        }
    }

    content.push_str("---\n\n");
    content.push_str("*Review this pull request and provide feedback or make changes.*\n");
    content
}

pub async fn load_gitea_pr_context(
    app: AppHandle,
    session_id: String,
    pr_number: u32,
    project_id: String,
) -> Result<LoadedGiteaPullRequestContext, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let repo_key = gitea_repo_key(&config);

    let pr = get_gitea_pr(app.clone(), project_id.clone(), pr_number).await?;
    let diff = get_gitea_pr_diff(app.clone(), project_id, pr_number)
        .await
        .unwrap_or(None);

    let ctx = GiteaPullRequestContext {
        number: pr.number,
        title: pr.title.clone(),
        body: pr.body,
        head_ref_name: pr.head_ref_name,
        base_ref_name: pr.base_ref_name,
        comments: pr.comments,
        reviews: pr.reviews.clone(),
        diff,
    };

    let contexts_dir = get_github_contexts_dir(&app)?;
    std::fs::create_dir_all(&contexts_dir)
        .map_err(|e| format!("Failed to create git-context directory: {e}"))?;

    let context_file = contexts_dir.join(format!("gitea-{repo_key}-pr-{pr_number}.md"));
    std::fs::write(&context_file, format_gitea_pr_context_markdown(&ctx))
        .map_err(|e| format!("Failed to write PR context file: {e}"))?;

    add_gitea_pr_reference(&app, &repo_key, pr_number, &session_id)?;

    Ok(LoadedGiteaPullRequestContext {
        number: pr.number,
        title: pr.title,
        comment_count: ctx.comments.len(),
        review_count: pr.reviews.len(),
        repo_owner: config.owner,
        repo_name: config.repo,
    })
}

pub async fn list_loaded_gitea_pr_contexts(
    app: AppHandle,
    session_id: String,
    worktree_id: Option<String>,
) -> Result<Vec<LoadedGiteaPullRequestContext>, String> {
    let mut pr_keys = get_session_gitea_pr_refs(&app, &session_id)?;
    if let Some(ref wt_id) = worktree_id {
        if let Ok(wt_keys) = get_session_gitea_pr_refs(&app, wt_id) {
            for key in wt_keys {
                if !pr_keys.contains(&key) {
                    pr_keys.push(key);
                }
            }
        }
    }

    if pr_keys.is_empty() {
        return Ok(vec![]);
    }

    let contexts_dir = get_github_contexts_dir(&app)?;
    let mut contexts = Vec::new();

    for key in pr_keys {
        if let Some((owner, repo, number)) = parse_gitea_context_key(&key) {
            let repo_key = format!("{owner}-{repo}");
            let context_file = contexts_dir.join(format!("gitea-{repo_key}-pr-{number}.md"));

            if let Ok(content) = std::fs::read_to_string(&context_file) {
                let title = content
                    .lines()
                    .next()
                    .and_then(|line| {
                        line.strip_prefix("# Gitea Pull Request #")
                            .and_then(|rest| rest.split_once(": "))
                            .map(|(_, title)| title.to_string())
                    })
                    .unwrap_or_else(|| format!("PR #{number}"));

                let comment_count = content
                    .find("## Comments")
                    .map(|start| content[start..].matches("### @").count())
                    .unwrap_or(0);

                let review_count = content
                    .find("## Reviews")
                    .map(|start| {
                        let reviews_section = &content[start..];
                        let end = reviews_section
                            .find("## Comments")
                            .unwrap_or(reviews_section.len());
                        reviews_section[..end].matches("### @").count()
                    })
                    .unwrap_or(0);

                contexts.push(LoadedGiteaPullRequestContext {
                    number,
                    title,
                    comment_count,
                    review_count,
                    repo_owner: owner,
                    repo_name: repo,
                });
            }
        }
    }

    contexts.sort_by_key(|c| c.number);
    Ok(contexts)
}

pub async fn remove_gitea_pr_context(
    app: AppHandle,
    session_id: String,
    pr_number: u32,
    project_id: String,
) -> Result<(), String> {
    let config = get_gitea_config(&app, &project_id)?;
    let repo_key = gitea_repo_key(&config);

    let is_orphaned = remove_gitea_pr_reference(&app, &repo_key, pr_number, &session_id)?;

    if is_orphaned {
        let contexts_dir = get_github_contexts_dir(&app)?;
        let context_file = contexts_dir.join(format!("gitea-{repo_key}-pr-{pr_number}.md"));
        if context_file.exists() {
            std::fs::remove_file(&context_file)
                .map_err(|e| format!("Failed to remove PR context file: {e}"))?;
        }
    }

    Ok(())
}

pub async fn get_gitea_pr_context_content(
    app: AppHandle,
    session_id: String,
    pr_number: u32,
    project_id: String,
) -> Result<String, String> {
    let config = get_gitea_config(&app, &project_id)?;
    let repo_key = gitea_repo_key(&config);

    let refs = get_session_gitea_pr_refs(&app, &session_id)?;
    let expected_key = format!("{repo_key}-{pr_number}");
    if !refs.contains(&expected_key) {
        return Err(format!("Session does not have PR #{pr_number} loaded"));
    }

    let contexts_dir = get_github_contexts_dir(&app)?;
    let context_file = contexts_dir.join(format!("gitea-{repo_key}-pr-{pr_number}.md"));
    if !context_file.exists() {
        return Err(format!("PR context file not found for PR #{pr_number}"));
    }
    std::fs::read_to_string(&context_file)
        .map_err(|e| format!("Failed to read PR context file: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_readable_branch_names() {
        assert_eq!(
            generate_branch_name_from_gitea_issue(42, "Fix the login bug!"),
            "issue-42-fix-the-login-bug"
        );
        assert_eq!(
            generate_branch_name_from_gitea_pr(7, "Add dark mode"),
            "pr-7-add-dark-mode"
        );
    }

    #[test]
    fn parses_context_key() {
        assert_eq!(
            parse_gitea_context_key("rysh-jean-gitea-test-42"),
            Some(("rysh".to_string(), "jean-gitea-test".to_string(), 42))
        );
        assert_eq!(parse_gitea_context_key("not-a-number"), None);
    }

    #[test]
    fn formats_issue_context_markdown() {
        let ctx = GiteaIssueContext {
            number: 1,
            title: "Testowe issue".to_string(),
            body: Some("Opis".to_string()),
            comments: vec![GiteaComment {
                body: "Komentarz".to_string(),
                author: GiteaAuthor {
                    login: "rysh".to_string(),
                },
                created_at: "2026-07-25T11:31:46Z".to_string(),
            }],
        };
        let content = format_gitea_issue_context_markdown(&ctx);
        assert!(content.contains("# Gitea Issue #1: Testowe issue"));
        assert!(content.contains("### @rysh (2026-07-25T11:31:46Z)"));
    }

    #[test]
    fn builds_api_urls_under_the_v1_prefix() {
        let url = gitea_api_url(
            "https://gitea.rysh",
            &["repos", "rysh", "jean-gitea-test", "issues"],
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://gitea.rysh/api/v1/repos/rysh/jean-gitea-test/issues"
        );
    }
}
