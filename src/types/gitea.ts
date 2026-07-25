/**
 * Gitea issue/PR/actions types, mirroring jean-core/src/projects/gitea_issues.rs
 * and gitea_actions.rs. All Rust structs use #[serde(rename_all = "camelCase")],
 * so — unlike types/github.ts — every field here is camelCase, including
 * timestamps (createdAt, not created_at).
 */

export interface GiteaLabel {
  name: string
  color: string
}

export interface GiteaAuthor {
  login: string
}

export interface GiteaIssue {
  number: number
  title: string
  body?: string
  state: string
  labels: GiteaLabel[]
  createdAt: string
  author: GiteaAuthor
}

export interface GiteaComment {
  body: string
  author: GiteaAuthor
  createdAt: string
}

export interface GiteaIssueDetail extends GiteaIssue {
  url: string
  comments: GiteaComment[]
}

/** Issue context to pass when creating a worktree */
export interface GiteaIssueContext {
  number: number
  title: string
  body?: string
  comments: GiteaComment[]
}

/** Loaded issue context info (from backend) */
export interface LoadedGiteaIssueContext {
  number: number
  title: string
  commentCount: number
  repoOwner: string
  repoName: string
}

// =============================================================================
// Gitea Pull Request Types
// =============================================================================

export interface GiteaPullRequest {
  number: number
  title: string
  body?: string
  state: string
  headRefName: string
  baseRefName: string
  isDraft: boolean
  createdAt: string
  author: GiteaAuthor
  labels: GiteaLabel[]
}

export interface GiteaReview {
  body: string
  state: string
  author: GiteaAuthor
  submittedAt?: string
}

/** Inline code review comment on specific diff lines */
export interface GiteaReviewComment {
  author: GiteaAuthor
  body: string
  createdAt: string
  diffHunk: string
  path: string
  line?: number
}

export interface GiteaPullRequestDetail extends GiteaPullRequest {
  comments: GiteaComment[]
  reviews: GiteaReview[]
}

/** PR context to pass when creating a worktree */
export interface GiteaPullRequestContext {
  number: number
  title: string
  body?: string
  headRefName: string
  baseRefName: string
  comments: GiteaComment[]
  reviews: GiteaReview[]
  diff?: string
}

/** Loaded PR context info (from backend) */
export interface LoadedGiteaPullRequestContext {
  number: number
  title: string
  commentCount: number
  reviewCount: number
  repoOwner: string
  repoName: string
}

// =============================================================================
// Gitea Actions Workflow Run Types
// =============================================================================

export interface GiteaWorkflowRun {
  id: number
  name: string
  status: string
  conclusion: string | null
  event: string
  headBranch: string
  createdAt: string
  url: string
}

export interface GiteaWorkflowRunsResult {
  runs: GiteaWorkflowRun[]
  failedCount: number
}
