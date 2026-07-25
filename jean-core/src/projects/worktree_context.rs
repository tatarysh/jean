//! Shared abstraction over the "context source" a worktree is created from
//! (a GitHub issue, GitHub PR, GitHub security alert, GitHub advisory, Linear
//! issue, or Sentry issue). Consolidates 3 patterns that used to be
//! duplicated once per provider inside `create_worktree`: branch name
//! generation, context markdown file naming/formatting, and field mapping
//! into `Worktree`/`WorktreeCreatingEvent`.
//!
//! # Known pre-existing asymmetries — PRESERVED, NOT FIXED
//!
//! These quirks predate this abstraction. They are intentionally preserved
//! exactly as they behaved before this file existed; do not "fix" them as a
//! side effect of touching this code.
//!
//! - GitHub-family variants (Issue/PullRequest/Security/Advisory) key their
//!   context filenames and reference lookups by a `repo_key` derived from
//!   `get_repo_identifier`, which is fallible. Linear/Sentry key by the
//!   Jean project name directly, unconditionally. See
//!   `context_file_name_github` vs `context_file_name_generic` and
//!   `add_reference_github` vs `add_reference_generic`.
//! - Sentry's context filename uses `SentryIssueContext::id`, while its
//!   branch name generator uses `SentryIssueContext::short_id` — two
//!   different identity fields from the same struct, used for two
//!   different purposes.
//! - Linear's context filename lowercases the issue identifier; the
//!   reference key registered via `add_linear_reference` does not.
//! - Sentry has no dedicated markdown formatter — `ctx.content`
//!   (pre-rendered elsewhere when the Sentry context was loaded) is used
//!   verbatim as the context file's content.
//! - `WorktreeCreatingEvent` has no fields for Linear/Sentry context at all
//!   (only issue/pr/security/advisory) — `apply_creating_event_fields` is a
//!   no-op for those two variants, matching the struct's existing shape.
//! - `Worktree` has no field for Sentry association at all — a
//!   Sentry-sourced worktree's link to its Sentry issue lives only in the
//!   context-references side file, never on the `Worktree` record.
//! - `create_worktree_from_existing_branch` and `checkout_pr` in
//!   `commands.rs` independently duplicate parts of this same
//!   field-mapping pattern. They are not wired to this enum — that's a
//!   separate, out-of-scope follow-up.

use super::github_issues::{
    add_advisory_reference, add_issue_reference, add_pr_reference, add_security_reference,
    format_advisory_context_markdown, format_issue_context_markdown, format_pr_context_markdown,
    format_security_context_markdown, generate_branch_name_from_advisory,
    generate_branch_name_from_issue, generate_branch_name_from_security_alert, AdvisoryContext,
    IssueContext, PullRequestContext, SecurityAlertContext,
};
use super::linear_issues::{
    add_linear_reference, format_linear_issue_context_markdown,
    generate_branch_name_from_linear_issue, linear_context_to_detail, LinearIssueContext,
};
use super::sentry_issues::{
    add_sentry_reference, generate_branch_name_from_sentry_issue, SentryIssueContext,
};
use super::types::{Worktree, WorktreeCreatingEvent};
use tauri::AppHandle;

/// The provider a worktree is being created from. Exactly one variant can
/// be present at a time, unlike the previous 6-separate-`Option` signature
/// this replaced (which allowed multiple to technically be set at once).
#[derive(Debug, Clone)]
pub enum WorktreeContextSource {
    Issue(IssueContext),
    PullRequest(PullRequestContext),
    Security(SecurityAlertContext),
    Advisory(AdvisoryContext),
    Linear(LinearIssueContext),
    Sentry(SentryIssueContext),
}

impl WorktreeContextSource {
    /// Generate the base branch name for this context source. Does not
    /// apply the uniqueness-suffix loop — that stays a single shared step
    /// at the call site, applied uniformly regardless of provider.
    pub fn generate_branch_name(&self) -> String {
        match self {
            WorktreeContextSource::Issue(ctx) => {
                generate_branch_name_from_issue(ctx.number, &ctx.title)
            }
            WorktreeContextSource::PullRequest(ctx) => {
                super::commands::generate_pr_worktree_name(ctx.number, &ctx.head_ref_name)
            }
            WorktreeContextSource::Security(ctx) => generate_branch_name_from_security_alert(
                ctx.number,
                &ctx.package_name,
                &ctx.summary,
            ),
            WorktreeContextSource::Advisory(ctx) => {
                generate_branch_name_from_advisory(&ctx.ghsa_id, &ctx.summary)
            }
            WorktreeContextSource::Linear(ctx) => {
                generate_branch_name_from_linear_issue(&ctx.identifier, &ctx.title)
            }
            WorktreeContextSource::Sentry(ctx) => {
                generate_branch_name_from_sentry_issue(&ctx.short_id, &ctx.title)
            }
        }
    }

    /// Context filename for the GitHub-family variants (Issue/PullRequest/
    /// Security/Advisory), keyed by `repo_key` (from `get_repo_identifier`).
    /// Panics if called on a Linear/Sentry variant — callers only reach this
    /// after already branching on which key scheme applies.
    pub fn context_file_name_github(&self, repo_key: &str) -> String {
        match self {
            WorktreeContextSource::Issue(ctx) => format!("{repo_key}-issue-{}.md", ctx.number),
            WorktreeContextSource::PullRequest(ctx) => {
                format!("{repo_key}-pr-{}.md", ctx.number)
            }
            WorktreeContextSource::Security(ctx) => {
                format!("{repo_key}-security-{}.md", ctx.number)
            }
            WorktreeContextSource::Advisory(ctx) => {
                format!("{repo_key}-advisory-{}.md", ctx.ghsa_id)
            }
            WorktreeContextSource::Linear(_) | WorktreeContextSource::Sentry(_) => {
                unreachable!("context_file_name_github called on a non-GitHub variant")
            }
        }
    }

    /// Context filename for the Linear/Sentry variants, keyed by the Jean
    /// project name (no repo identifier involved). Panics if called on a
    /// GitHub-family variant.
    pub fn context_file_name_generic(&self, project_name: &str) -> String {
        match self {
            WorktreeContextSource::Linear(ctx) => {
                format!("{project_name}-linear-{}.md", ctx.identifier.to_lowercase())
            }
            WorktreeContextSource::Sentry(ctx) => {
                format!("{project_name}-sentry-{}.md", ctx.id)
            }
            _ => unreachable!("context_file_name_generic called on a GitHub-family variant"),
        }
    }

    /// Markdown content to write to the context file. For PullRequest, the
    /// caller is expected to pass a diff-populated context (the diff-fetch
    /// itself is an impure, network-touching step that stays explicit at
    /// the call site, not hidden inside this method).
    pub fn context_markdown(&self) -> String {
        match self {
            WorktreeContextSource::Issue(ctx) => format_issue_context_markdown(ctx),
            WorktreeContextSource::PullRequest(ctx) => format_pr_context_markdown(ctx),
            WorktreeContextSource::Security(ctx) => format_security_context_markdown(ctx),
            WorktreeContextSource::Advisory(ctx) => format_advisory_context_markdown(ctx),
            WorktreeContextSource::Linear(ctx) => {
                format_linear_issue_context_markdown(&linear_context_to_detail(ctx))
            }
            WorktreeContextSource::Sentry(ctx) => ctx.content.clone(),
        }
    }

    /// Register a session/worktree reference for the GitHub-family variants.
    pub fn add_reference_github(
        &self,
        app: &AppHandle,
        repo_key: &str,
        worktree_id: &str,
    ) -> Result<(), String> {
        match self {
            WorktreeContextSource::Issue(ctx) => {
                add_issue_reference(app, repo_key, ctx.number, worktree_id)
            }
            WorktreeContextSource::PullRequest(ctx) => {
                add_pr_reference(app, repo_key, ctx.number, worktree_id)
            }
            WorktreeContextSource::Security(ctx) => {
                add_security_reference(app, repo_key, ctx.number, worktree_id)
            }
            WorktreeContextSource::Advisory(ctx) => {
                add_advisory_reference(app, repo_key, &ctx.ghsa_id, worktree_id)
            }
            WorktreeContextSource::Linear(_) | WorktreeContextSource::Sentry(_) => {
                unreachable!("add_reference_github called on a non-GitHub variant")
            }
        }
    }

    /// Register a session/worktree reference for the Linear/Sentry variants.
    pub fn add_reference_generic(
        &self,
        app: &AppHandle,
        project_name: &str,
        worktree_id: &str,
    ) -> Result<(), String> {
        match self {
            WorktreeContextSource::Linear(ctx) => {
                add_linear_reference(app, project_name, &ctx.identifier, worktree_id)
            }
            WorktreeContextSource::Sentry(ctx) => {
                add_sentry_reference(app, project_name, &ctx.id, worktree_id)
            }
            _ => unreachable!("add_reference_generic called on a GitHub-family variant"),
        }
    }

    /// Set the context fields on a `WorktreeCreatingEvent`. Linear/Sentry
    /// are no-ops — see the module-level "PRESERVED, NOT FIXED" notes.
    pub fn apply_creating_event_fields(&self, event: &mut WorktreeCreatingEvent) {
        match self {
            WorktreeContextSource::Issue(ctx) => {
                event.issue_number = Some(ctx.number as u64);
            }
            WorktreeContextSource::PullRequest(ctx) => {
                event.pr_number = Some(ctx.number as u64);
            }
            WorktreeContextSource::Security(ctx) => {
                event.security_alert_number = Some(ctx.number as u64);
            }
            WorktreeContextSource::Advisory(ctx) => {
                event.advisory_ghsa_id = Some(ctx.ghsa_id.clone());
            }
            // Pre-existing gap: WorktreeCreatingEvent has no linear/sentry fields.
            WorktreeContextSource::Linear(_) | WorktreeContextSource::Sentry(_) => {}
        }
    }

    /// Set the context fields on a `Worktree`. Sentry is a no-op — see the
    /// module-level "PRESERVED, NOT FIXED" notes (no Sentry field exists on
    /// `Worktree` at all).
    pub fn apply_worktree_fields(&self, worktree: &mut Worktree) {
        match self {
            WorktreeContextSource::Issue(ctx) => {
                worktree.issue_number = Some(ctx.number);
            }
            WorktreeContextSource::PullRequest(ctx) => {
                worktree.pr_number = Some(ctx.number);
            }
            WorktreeContextSource::Security(ctx) => {
                worktree.security_alert_number = Some(ctx.number);
                worktree.security_alert_url = ctx.html_url.clone();
            }
            WorktreeContextSource::Advisory(ctx) => {
                worktree.advisory_ghsa_id = Some(ctx.ghsa_id.clone());
                worktree.advisory_url = ctx.html_url.clone();
            }
            WorktreeContextSource::Linear(ctx) => {
                worktree.linear_issue_identifier = Some(ctx.identifier.clone());
            }
            // Pre-existing gap: Worktree has no field for Sentry association.
            WorktreeContextSource::Sentry(_) => {}
        }
    }

    /// Borrow the inner `PullRequestContext` if this is a PullRequest
    /// variant. Used by PR-only call-site logic (base branch fallback,
    /// diff fetch, temp-branch checkout, branch-exists exemption,
    /// auto-pull suppression) that doesn't fit a uniform per-variant method.
    pub fn pr_context(&self) -> Option<&PullRequestContext> {
        match self {
            WorktreeContextSource::PullRequest(ctx) => Some(ctx),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::SessionType;

    fn sample_issue() -> IssueContext {
        IssueContext {
            number: 42,
            title: "Fix the login bug".to_string(),
            body: None,
            comments: vec![],
        }
    }

    fn sample_pr() -> PullRequestContext {
        PullRequestContext {
            number: 7,
            title: "Add feature".to_string(),
            body: None,
            head_ref_name: "feature-branch".to_string(),
            base_ref_name: "main".to_string(),
            comments: vec![],
            reviews: vec![],
            diff: None,
        }
    }

    fn sample_security() -> SecurityAlertContext {
        SecurityAlertContext {
            number: 3,
            package_name: "lodash".to_string(),
            package_ecosystem: "npm".to_string(),
            severity: "high".to_string(),
            summary: "Prototype pollution".to_string(),
            description: "A prototype pollution vulnerability.".to_string(),
            ghsa_id: "GHSA-abcd-1234-efgh".to_string(),
            cve_id: None,
            manifest_path: "package.json".to_string(),
            html_url: Some("https://github.com/example/repo/security/1".to_string()),
        }
    }

    fn sample_advisory() -> AdvisoryContext {
        AdvisoryContext {
            ghsa_id: "GHSA-wxyz-5678-ijkl".to_string(),
            severity: "critical".to_string(),
            summary: "Remote code execution".to_string(),
            description: "An RCE vulnerability.".to_string(),
            cve_id: None,
            vulnerabilities: vec![],
            html_url: Some("https://github.com/example/repo/advisories/1".to_string()),
        }
    }

    fn sample_linear() -> LinearIssueContext {
        LinearIssueContext {
            id: "linear-abc123".to_string(),
            identifier: "ENG-45".to_string(),
            title: "Fix the thing".to_string(),
            description: None,
            comments: vec![],
        }
    }

    fn sample_sentry() -> SentryIssueContext {
        SentryIssueContext {
            id: "999888777".to_string(),
            short_id: "PROJ-7Q".to_string(),
            title: "NullPointerException in handler".to_string(),
            permalink: "https://sentry.io/organizations/example/issues/999888777/".to_string(),
            content: "# Sentry Issue PROJ-7Q\n\nNullPointerException in handler\n".to_string(),
        }
    }

    fn blank_worktree() -> Worktree {
        Worktree {
            id: "wt-id".to_string(),
            project_id: "proj-id".to_string(),
            name: "some-worktree".to_string(),
            path: "/tmp/some-worktree".to_string(),
            branch: "some-worktree".to_string(),
            base_branch: None,
            base_remote: None,
            created_at: 0,
            setup_output: None,
            setup_script: None,
            setup_success: None,
            session_type: SessionType::Worktree,
            pr_number: None,
            pr_url: None,
            issue_number: None,
            linear_issue_identifier: None,
            security_alert_number: None,
            security_alert_url: None,
            advisory_ghsa_id: None,
            advisory_url: None,
            cached_pr_status: None,
            cached_check_status: None,
            cached_behind_count: None,
            cached_ahead_count: None,
            cached_status_at: None,
            cached_uncommitted_added: None,
            cached_uncommitted_removed: None,
            cached_branch_diff_added: None,
            cached_branch_diff_removed: None,
            cached_base_branch_ahead_count: None,
            cached_base_branch_behind_count: None,
            cached_worktree_ahead_count: None,
            cached_unpushed_count: None,
            pr_push_remote: None,
            pr_push_branch: None,
            order: 0,
            origin: None,
            labels: vec![],
            label: None,
            archived_at: None,
            last_opened_at: None,
        }
    }

    fn blank_creating_event() -> WorktreeCreatingEvent {
        WorktreeCreatingEvent {
            id: "wt-id".to_string(),
            project_id: "proj-id".to_string(),
            name: "some-worktree".to_string(),
            path: "/tmp/some-worktree".to_string(),
            branch: "some-worktree".to_string(),
            pr_number: None,
            issue_number: None,
            security_alert_number: None,
            advisory_ghsa_id: None,
            origin: None,
            auto_open_in_jean: true,
        }
    }

    // --- generate_branch_name: cross-checked against the underlying free functions ---

    #[test]
    fn generate_branch_name_issue_matches_free_function() {
        let ctx = sample_issue();
        let source = WorktreeContextSource::Issue(ctx.clone());
        assert_eq!(
            source.generate_branch_name(),
            generate_branch_name_from_issue(ctx.number, &ctx.title)
        );
    }

    #[test]
    fn generate_branch_name_pr_matches_free_function() {
        let ctx = sample_pr();
        let source = WorktreeContextSource::PullRequest(ctx.clone());
        assert_eq!(
            source.generate_branch_name(),
            super::super::commands::generate_pr_worktree_name(ctx.number, &ctx.head_ref_name)
        );
    }

    #[test]
    fn generate_branch_name_security_matches_free_function() {
        let ctx = sample_security();
        let source = WorktreeContextSource::Security(ctx.clone());
        assert_eq!(
            source.generate_branch_name(),
            generate_branch_name_from_security_alert(ctx.number, &ctx.package_name, &ctx.summary)
        );
    }

    #[test]
    fn generate_branch_name_advisory_matches_free_function() {
        let ctx = sample_advisory();
        let source = WorktreeContextSource::Advisory(ctx.clone());
        assert_eq!(
            source.generate_branch_name(),
            generate_branch_name_from_advisory(&ctx.ghsa_id, &ctx.summary)
        );
    }

    #[test]
    fn generate_branch_name_linear_matches_free_function() {
        let ctx = sample_linear();
        let source = WorktreeContextSource::Linear(ctx.clone());
        assert_eq!(
            source.generate_branch_name(),
            generate_branch_name_from_linear_issue(&ctx.identifier, &ctx.title)
        );
    }

    #[test]
    fn generate_branch_name_sentry_matches_free_function() {
        let ctx = sample_sentry();
        let source = WorktreeContextSource::Sentry(ctx.clone());
        assert_eq!(
            source.generate_branch_name(),
            generate_branch_name_from_sentry_issue(&ctx.short_id, &ctx.title)
        );
    }

    // --- context_file_name: exact filename shapes, including the known quirks ---

    #[test]
    fn context_file_name_github_shapes() {
        let repo_key = "owner-repo";
        assert_eq!(
            WorktreeContextSource::Issue(sample_issue()).context_file_name_github(repo_key),
            "owner-repo-issue-42.md"
        );
        assert_eq!(
            WorktreeContextSource::PullRequest(sample_pr()).context_file_name_github(repo_key),
            "owner-repo-pr-7.md"
        );
        assert_eq!(
            WorktreeContextSource::Security(sample_security()).context_file_name_github(repo_key),
            "owner-repo-security-3.md"
        );
        assert_eq!(
            WorktreeContextSource::Advisory(sample_advisory()).context_file_name_github(repo_key),
            "owner-repo-advisory-GHSA-wxyz-5678-ijkl.md"
        );
    }

    #[test]
    fn context_file_name_linear_lowercases_identifier() {
        // Quirk: the filename lowercases the identifier; the reference key
        // (add_linear_reference) does NOT — see add_reference_generic tests.
        let ctx = LinearIssueContext {
            identifier: "ENG-45".to_string(),
            ..sample_linear()
        };
        assert_eq!(
            WorktreeContextSource::Linear(ctx).context_file_name_generic("myproject"),
            "myproject-linear-eng-45.md"
        );
    }

    #[test]
    fn context_file_name_sentry_uses_id_not_short_id() {
        // Quirk: filename uses `id`; generate_branch_name uses `short_id`.
        // Pin both uses down together so a future edit can't quietly unify them.
        let ctx = SentryIssueContext {
            id: "999888777".to_string(),
            short_id: "PROJ-7Q".to_string(),
            ..sample_sentry()
        };
        let source = WorktreeContextSource::Sentry(ctx);
        assert_eq!(
            source.context_file_name_generic("myproject"),
            "myproject-sentry-999888777.md"
        );
        assert!(source.generate_branch_name().contains("proj-7q"));
        assert!(!source.generate_branch_name().contains("999888777"));
    }

    // --- context_markdown: delegate-and-compare, plus Sentry's verbatim passthrough ---

    #[test]
    fn context_markdown_delegates_to_formatters() {
        let issue = sample_issue();
        assert_eq!(
            WorktreeContextSource::Issue(issue.clone()).context_markdown(),
            format_issue_context_markdown(&issue)
        );

        let pr = sample_pr();
        assert_eq!(
            WorktreeContextSource::PullRequest(pr.clone()).context_markdown(),
            format_pr_context_markdown(&pr)
        );

        let security = sample_security();
        assert_eq!(
            WorktreeContextSource::Security(security.clone()).context_markdown(),
            format_security_context_markdown(&security)
        );

        let advisory = sample_advisory();
        assert_eq!(
            WorktreeContextSource::Advisory(advisory.clone()).context_markdown(),
            format_advisory_context_markdown(&advisory)
        );

        let linear = sample_linear();
        assert_eq!(
            WorktreeContextSource::Linear(linear.clone()).context_markdown(),
            format_linear_issue_context_markdown(&linear_context_to_detail(&linear))
        );
    }

    #[test]
    fn context_markdown_sentry_is_verbatim_content() {
        let ctx = sample_sentry();
        let source = WorktreeContextSource::Sentry(ctx.clone());
        assert_eq!(source.context_markdown(), ctx.content);
    }

    // --- apply_worktree_fields: only the expected field(s) set, Sentry is a no-op ---

    #[test]
    fn apply_worktree_fields_issue() {
        let mut wt = blank_worktree();
        WorktreeContextSource::Issue(sample_issue()).apply_worktree_fields(&mut wt);
        assert_eq!(wt.issue_number, Some(42));
        assert_eq!(wt.pr_number, None);
        assert_eq!(wt.linear_issue_identifier, None);
        assert_eq!(wt.security_alert_number, None);
        assert_eq!(wt.advisory_ghsa_id, None);
    }

    #[test]
    fn apply_worktree_fields_pr() {
        let mut wt = blank_worktree();
        WorktreeContextSource::PullRequest(sample_pr()).apply_worktree_fields(&mut wt);
        assert_eq!(wt.pr_number, Some(7));
        assert_eq!(wt.issue_number, None);
    }

    #[test]
    fn apply_worktree_fields_security() {
        let mut wt = blank_worktree();
        let ctx = sample_security();
        let html_url = ctx.html_url.clone();
        WorktreeContextSource::Security(ctx).apply_worktree_fields(&mut wt);
        assert_eq!(wt.security_alert_number, Some(3));
        assert_eq!(wt.security_alert_url, html_url);
    }

    #[test]
    fn apply_worktree_fields_advisory() {
        let mut wt = blank_worktree();
        let ctx = sample_advisory();
        let html_url = ctx.html_url.clone();
        WorktreeContextSource::Advisory(ctx).apply_worktree_fields(&mut wt);
        assert_eq!(wt.advisory_ghsa_id, Some("GHSA-wxyz-5678-ijkl".to_string()));
        assert_eq!(wt.advisory_url, html_url);
    }

    #[test]
    fn apply_worktree_fields_linear() {
        let mut wt = blank_worktree();
        WorktreeContextSource::Linear(sample_linear()).apply_worktree_fields(&mut wt);
        assert_eq!(wt.linear_issue_identifier, Some("ENG-45".to_string()));
    }

    #[test]
    fn apply_worktree_fields_sentry_is_noop() {
        // Pre-existing gap: Worktree has no Sentry field at all.
        let mut wt = blank_worktree();
        WorktreeContextSource::Sentry(sample_sentry()).apply_worktree_fields(&mut wt);
        assert_eq!(wt.pr_number, None);
        assert_eq!(wt.issue_number, None);
        assert_eq!(wt.linear_issue_identifier, None);
        assert_eq!(wt.security_alert_number, None);
        assert_eq!(wt.advisory_ghsa_id, None);
    }

    // --- apply_creating_event_fields: Linear/Sentry are no-ops (pre-existing gap) ---

    #[test]
    fn apply_creating_event_fields_issue() {
        let mut event = blank_creating_event();
        WorktreeContextSource::Issue(sample_issue()).apply_creating_event_fields(&mut event);
        assert_eq!(event.issue_number, Some(42));
    }

    #[test]
    fn apply_creating_event_fields_pr() {
        let mut event = blank_creating_event();
        WorktreeContextSource::PullRequest(sample_pr()).apply_creating_event_fields(&mut event);
        assert_eq!(event.pr_number, Some(7));
    }

    #[test]
    fn apply_creating_event_fields_security() {
        let mut event = blank_creating_event();
        WorktreeContextSource::Security(sample_security())
            .apply_creating_event_fields(&mut event);
        assert_eq!(event.security_alert_number, Some(3));
    }

    #[test]
    fn apply_creating_event_fields_advisory() {
        let mut event = blank_creating_event();
        WorktreeContextSource::Advisory(sample_advisory())
            .apply_creating_event_fields(&mut event);
        assert_eq!(
            event.advisory_ghsa_id,
            Some("GHSA-wxyz-5678-ijkl".to_string())
        );
    }

    #[test]
    fn apply_creating_event_fields_linear_and_sentry_are_noops() {
        let mut event = blank_creating_event();
        WorktreeContextSource::Linear(sample_linear()).apply_creating_event_fields(&mut event);
        assert_eq!(event.pr_number, None);
        assert_eq!(event.issue_number, None);
        assert_eq!(event.security_alert_number, None);
        assert_eq!(event.advisory_ghsa_id, None);

        let mut event = blank_creating_event();
        WorktreeContextSource::Sentry(sample_sentry()).apply_creating_event_fields(&mut event);
        assert_eq!(event.pr_number, None);
        assert_eq!(event.issue_number, None);
        assert_eq!(event.security_alert_number, None);
        assert_eq!(event.advisory_ghsa_id, None);
    }

    // --- pr_context ---

    #[test]
    fn pr_context_only_present_on_pull_request_variant() {
        assert!(WorktreeContextSource::PullRequest(sample_pr())
            .pr_context()
            .is_some());
        assert!(WorktreeContextSource::Issue(sample_issue())
            .pr_context()
            .is_none());
        assert!(WorktreeContextSource::Sentry(sample_sentry())
            .pr_context()
            .is_none());
    }

    // Note: add_reference_github / add_reference_generic are not unit-tested
    // here — they call load_context_references/save_context_references,
    // which need a live AppHandle to resolve the app data directory. No
    // AppHandle-mocking pattern exists elsewhere in this codebase to reuse.
    // Covered instead by the manual verification checklist (see the
    // create_worktree refactor plan/PR description).
}
