import { useQuery } from '@tanstack/react-query'
import { invoke } from '@/lib/transport'
import { logger } from '@/lib/logger'
import type {
  GiteaLabel,
  GiteaIssue,
  GiteaIssueDetail,
  GiteaPullRequest,
  GiteaPullRequestDetail,
  GiteaReviewComment,
  LoadedGiteaIssueContext,
  LoadedGiteaPullRequestContext,
  GiteaWorkflowRunsResult,
} from '@/types/gitea'
import { isTauri } from './projects'
import { parseLabelQuery, parseItemNumber, mergeWithSearchResults, prependExactMatch } from './github'

// Reuse of provider-agnostic helpers from services/github: parseLabelQuery, parseItemNumber,
// mergeWithSearchResults, and prependExactMatch operate purely on generic {number}-shaped
// arrays/strings, so duplicating them here would add nothing but drift.
export { parseLabelQuery, parseItemNumber, mergeWithSearchResults, prependExactMatch }

function getErrorMessage(error: unknown): string {
  if (!error) return ''
  return error instanceof Error ? error.message : String(error)
}

/**
 * True when the error indicates missing per-project Gitea settings (URL, token,
 * owner, or repo) rather than an authentication failure — surfaced by
 * `get_gitea_config` in gitea_issues.rs when a required field is unset.
 */
export function isGiteaConfigError(error: unknown): boolean {
  const lower = getErrorMessage(error).toLowerCase()
  if (!lower) return false
  return (
    lower.includes('no gitea instance url configured') ||
    lower.includes('no gitea access token configured') ||
    lower.includes('no gitea repository owner configured') ||
    lower.includes('no gitea repository name configured')
  )
}

/**
 * True when the error indicates an invalid/expired Gitea access token (HTTP 401/403),
 * as opposed to a config error or a generic API failure.
 */
export function isGiteaAuthError(error: unknown): boolean {
  if (!error || isGiteaConfigError(error)) return false
  const lower = getErrorMessage(error).toLowerCase()
  return (
    lower.includes('access token is invalid') ||
    lower.includes('missing required permissions')
  )
}

// Query keys for Gitea
export const giteaQueryKeys = {
  all: ['gitea'] as const,
  issues: (projectId: string, state: string) =>
    [...giteaQueryKeys.all, 'issues', projectId, state] as const,
  issue: (projectId: string, issueNumber: number) =>
    [...giteaQueryKeys.all, 'issue', projectId, issueNumber] as const,
  issueSearch: (projectId: string, query: string) =>
    [...giteaQueryKeys.all, 'issue-search', projectId, query] as const,
  issueByNumber: (projectId: string, number: number) =>
    [...giteaQueryKeys.all, 'issue-by-number', projectId, number] as const,
  prs: (projectId: string, state: string) =>
    [...giteaQueryKeys.all, 'prs', projectId, state] as const,
  pr: (projectId: string, prNumber: number) =>
    [...giteaQueryKeys.all, 'pr', projectId, prNumber] as const,
  prSearch: (projectId: string, query: string) =>
    [...giteaQueryKeys.all, 'pr-search', projectId, query] as const,
  prByNumber: (projectId: string, number: number) =>
    [...giteaQueryKeys.all, 'pr-by-number', projectId, number] as const,
  prReviewComments: (projectId: string, prNumber: number) =>
    [...giteaQueryKeys.all, 'pr-review-comments', projectId, prNumber] as const,
  workflowRuns: (projectId: string, branch?: string) =>
    [...giteaQueryKeys.all, 'workflow-runs', projectId, branch ?? ''] as const,
  loadedIssueContexts: (sessionId: string, worktreeId?: string | null) =>
    worktreeId
      ? ([...giteaQueryKeys.all, 'loaded-issue-contexts', sessionId, worktreeId] as const)
      : ([...giteaQueryKeys.all, 'loaded-issue-contexts', sessionId] as const),
  loadedPrContexts: (sessionId: string, worktreeId?: string | null) =>
    worktreeId
      ? ([...giteaQueryKeys.all, 'loaded-pr-contexts', sessionId, worktreeId] as const)
      : ([...giteaQueryKeys.all, 'loaded-pr-contexts', sessionId] as const),
}

// =============================================================================
// Issues
// =============================================================================

export function useGiteaIssues(
  projectId: string | null,
  state: 'open' | 'closed' | 'all' = 'open',
  options?: { enabled?: boolean; staleTime?: number }
) {
  return useQuery({
    queryKey: giteaQueryKeys.issues(projectId ?? '', state),
    queryFn: async (): Promise<GiteaIssue[]> => {
      if (!isTauri() || !projectId) return []
      try {
        logger.debug('Fetching Gitea issues', { projectId, state })
        return await invoke<GiteaIssue[]>('list_gitea_issues', { projectId, state })
      } catch (error) {
        logger.error('Failed to load Gitea issues', { error, projectId })
        throw error
      }
    },
    enabled: (options?.enabled ?? true) && !!projectId,
    staleTime: options?.staleTime ?? 1000 * 60 * 2,
    gcTime: 1000 * 60 * 10,
    retry: 1,
  })
}

export function useSearchGiteaIssues(projectId: string | null, query: string) {
  return useQuery({
    queryKey: giteaQueryKeys.issueSearch(projectId ?? '', query),
    queryFn: async (): Promise<GiteaIssue[]> => {
      if (!isTauri() || !projectId || !query) return []
      try {
        return await invoke<GiteaIssue[]>('search_gitea_issues', { projectId, query })
      } catch (error) {
        logger.error('Failed to search Gitea issues', { error, projectId, query })
        throw error
      }
    },
    enabled: !!projectId && query.length >= 2,
    staleTime: 1000 * 30,
    gcTime: 1000 * 60 * 5,
    retry: 0,
  })
}

export function useGetGiteaIssueByNumber(projectId: string | null, query: string) {
  const itemNumber = parseItemNumber(query)
  return useQuery({
    queryKey: giteaQueryKeys.issueByNumber(projectId ?? '', itemNumber ?? 0),
    queryFn: async (): Promise<GiteaIssue | null> => {
      if (!isTauri() || !projectId || !itemNumber) return null
      try {
        return await invoke<GiteaIssue>('get_gitea_issue_by_number', {
          projectId,
          issueNumber: itemNumber,
        })
      } catch {
        return null
      }
    },
    enabled: !!projectId && itemNumber !== null,
    staleTime: 1000 * 30,
    gcTime: 1000 * 60 * 5,
    retry: 0,
  })
}

export function useGiteaIssue(projectId: string | null, issueNumber: number | null) {
  return useQuery({
    queryKey: giteaQueryKeys.issue(projectId ?? '', issueNumber ?? 0),
    queryFn: async (): Promise<GiteaIssueDetail> => {
      if (!isTauri() || !projectId || !issueNumber) {
        throw new Error('Missing required parameters')
      }
      return await invoke<GiteaIssueDetail>('get_gitea_issue', { projectId, issueNumber })
    },
    enabled: !!projectId && !!issueNumber,
    staleTime: 1000 * 60 * 5,
    gcTime: 1000 * 60 * 15,
  })
}

export function filterGiteaIssues(issues: GiteaIssue[], query: string): GiteaIssue[] {
  if (!query.trim()) return issues
  const { labels, textQuery } = parseLabelQuery(query)
  const lowerQuery = textQuery.toLowerCase()

  return issues.filter(issue => {
    if (labels.length > 0) {
      const issueLabels = issue.labels.map((l: GiteaLabel) => l.name.toLowerCase())
      if (!labels.every(l => issueLabels.some(il => il.includes(l)))) return false
    }
    if (!lowerQuery) return true
    const numberQuery = lowerQuery.replace(/^#/, '')
    if (issue.number.toString().includes(numberQuery)) return true
    if (issue.title.toLowerCase().includes(lowerQuery)) return true
    if (issue.body?.toLowerCase().includes(lowerQuery)) return true
    if (issue.labels.some(l => l.name.toLowerCase().includes(lowerQuery))) return true
    return false
  })
}

export function useLoadedGiteaIssueContexts(
  sessionId: string | null,
  worktreeId?: string | null
) {
  return useQuery({
    queryKey: giteaQueryKeys.loadedIssueContexts(sessionId ?? '', worktreeId),
    queryFn: async (): Promise<LoadedGiteaIssueContext[]> => {
      if (!isTauri() || !sessionId) return []
      try {
        return await invoke<LoadedGiteaIssueContext[]>('list_loaded_gitea_issue_contexts', {
          sessionId,
          worktreeId: worktreeId ?? undefined,
        })
      } catch (error) {
        logger.error('Failed to load Gitea issue contexts', { error, sessionId })
        throw error
      }
    },
    enabled: !!sessionId,
    staleTime: 1000 * 60,
    gcTime: 1000 * 60 * 5,
  })
}

export async function loadGiteaIssueContext(
  sessionId: string,
  issueNumber: number,
  projectId: string
): Promise<LoadedGiteaIssueContext> {
  return invoke<LoadedGiteaIssueContext>('load_gitea_issue_context', {
    sessionId,
    issueNumber,
    projectId,
  })
}

export async function removeGiteaIssueContext(
  sessionId: string,
  issueNumber: number,
  projectId: string
): Promise<void> {
  return invoke('remove_gitea_issue_context', { sessionId, issueNumber, projectId })
}

export async function getGiteaIssueContextContent(
  sessionId: string,
  issueNumber: number,
  projectId: string
): Promise<string> {
  return invoke<string>('get_gitea_issue_context_content', {
    sessionId,
    issueNumber,
    projectId,
  })
}

// =============================================================================
// Pull Requests
// =============================================================================

export function useGiteaPRs(
  projectId: string | null,
  state: 'open' | 'closed' | 'all' = 'open',
  options?: { enabled?: boolean; staleTime?: number }
) {
  return useQuery({
    queryKey: giteaQueryKeys.prs(projectId ?? '', state),
    queryFn: async (): Promise<GiteaPullRequest[]> => {
      if (!isTauri() || !projectId) return []
      try {
        logger.debug('Fetching Gitea PRs', { projectId, state })
        return await invoke<GiteaPullRequest[]>('list_gitea_prs', { projectId, state })
      } catch (error) {
        logger.error('Failed to load Gitea PRs', { error, projectId })
        throw error
      }
    },
    enabled: (options?.enabled ?? true) && !!projectId,
    staleTime: options?.staleTime ?? 1000 * 60 * 2,
    gcTime: 1000 * 60 * 10,
    retry: 1,
  })
}

export function useSearchGiteaPRs(projectId: string | null, query: string) {
  return useQuery({
    queryKey: giteaQueryKeys.prSearch(projectId ?? '', query),
    queryFn: async (): Promise<GiteaPullRequest[]> => {
      if (!isTauri() || !projectId || !query) return []
      try {
        return await invoke<GiteaPullRequest[]>('search_gitea_prs', { projectId, query })
      } catch (error) {
        logger.error('Failed to search Gitea PRs', { error, projectId, query })
        throw error
      }
    },
    enabled: !!projectId && query.length >= 2,
    staleTime: 1000 * 30,
    gcTime: 1000 * 60 * 5,
    retry: 0,
  })
}

export function useGetGiteaPRByNumber(projectId: string | null, query: string) {
  const itemNumber = parseItemNumber(query)
  return useQuery({
    queryKey: giteaQueryKeys.prByNumber(projectId ?? '', itemNumber ?? 0),
    queryFn: async (): Promise<GiteaPullRequest | null> => {
      if (!isTauri() || !projectId || !itemNumber) return null
      try {
        return await invoke<GiteaPullRequest>('get_gitea_pr_by_number', {
          projectId,
          prNumber: itemNumber,
        })
      } catch {
        return null
      }
    },
    enabled: !!projectId && itemNumber !== null,
    staleTime: 1000 * 30,
    gcTime: 1000 * 60 * 5,
    retry: 0,
  })
}

export function useGiteaPR(projectId: string | null, prNumber: number | null) {
  return useQuery({
    queryKey: giteaQueryKeys.pr(projectId ?? '', prNumber ?? 0),
    queryFn: async (): Promise<GiteaPullRequestDetail> => {
      if (!isTauri() || !projectId || !prNumber) {
        throw new Error('Missing required parameters')
      }
      return await invoke<GiteaPullRequestDetail>('get_gitea_pr', { projectId, prNumber })
    },
    enabled: !!projectId && !!prNumber,
    staleTime: 1000 * 60 * 5,
    gcTime: 1000 * 60 * 15,
  })
}

export function useGiteaPRReviewComments(projectId: string | null, prNumber: number | null) {
  return useQuery({
    queryKey: giteaQueryKeys.prReviewComments(projectId ?? '', prNumber ?? 0),
    queryFn: async (): Promise<GiteaReviewComment[]> => {
      if (!isTauri() || !projectId || !prNumber) return []
      return await invoke<GiteaReviewComment[]>('get_gitea_pr_review_comments', {
        projectId,
        prNumber,
      })
    },
    enabled: !!projectId && !!prNumber,
    staleTime: 1000 * 60 * 5,
    gcTime: 1000 * 60 * 15,
  })
}

export function filterGiteaPRs(prs: GiteaPullRequest[], query: string): GiteaPullRequest[] {
  if (!query.trim()) return prs
  const { labels, textQuery } = parseLabelQuery(query)
  const lowerQuery = textQuery.toLowerCase()

  return prs.filter(pr => {
    if (labels.length > 0) {
      const prLabels = pr.labels.map((l: GiteaLabel) => l.name.toLowerCase())
      if (!labels.every(l => prLabels.some(pl => pl.includes(l)))) return false
    }
    if (!lowerQuery) return true
    const numberQuery = lowerQuery.replace(/^#/, '')
    if (pr.number.toString().includes(numberQuery)) return true
    if (pr.title.toLowerCase().includes(lowerQuery)) return true
    if (pr.body?.toLowerCase().includes(lowerQuery)) return true
    if (pr.headRefName.toLowerCase().includes(lowerQuery)) return true
    if (pr.labels.some(l => l.name.toLowerCase().includes(lowerQuery))) return true
    return false
  })
}

export function useLoadedGiteaPRContexts(sessionId: string | null, worktreeId?: string | null) {
  return useQuery({
    queryKey: giteaQueryKeys.loadedPrContexts(sessionId ?? '', worktreeId),
    queryFn: async (): Promise<LoadedGiteaPullRequestContext[]> => {
      if (!isTauri() || !sessionId) return []
      try {
        return await invoke<LoadedGiteaPullRequestContext[]>('list_loaded_gitea_pr_contexts', {
          sessionId,
          worktreeId: worktreeId ?? undefined,
        })
      } catch (error) {
        logger.error('Failed to load Gitea PR contexts', { error, sessionId })
        throw error
      }
    },
    enabled: !!sessionId,
    staleTime: 1000 * 60,
    gcTime: 1000 * 60 * 5,
  })
}

export async function loadGiteaPRContext(
  sessionId: string,
  prNumber: number,
  projectId: string
): Promise<LoadedGiteaPullRequestContext> {
  return invoke<LoadedGiteaPullRequestContext>('load_gitea_pr_context', {
    sessionId,
    prNumber,
    projectId,
  })
}

export async function removeGiteaPRContext(
  sessionId: string,
  prNumber: number,
  projectId: string
): Promise<void> {
  return invoke('remove_gitea_pr_context', { sessionId, prNumber, projectId })
}

export async function getGiteaPRContextContent(
  sessionId: string,
  prNumber: number,
  projectId: string
): Promise<string> {
  return invoke<string>('get_gitea_pr_context_content', { sessionId, prNumber, projectId })
}

// =============================================================================
// Gitea Actions Workflow Runs
// =============================================================================

export interface GiteaConnectionStatus {
  fullName: string
  htmlUrl: string
}

/** Verify the project's Gitea URL, token, owner, and repo by fetching the repository. */
export async function testGiteaConnection(
  projectId: string
): Promise<GiteaConnectionStatus> {
  return invoke<GiteaConnectionStatus>('test_gitea_connection', { projectId })
}

export function useGiteaWorkflowRuns(
  projectId: string | null,
  branch?: string,
  options?: { enabled?: boolean; staleTime?: number }
) {
  return useQuery({
    queryKey: giteaQueryKeys.workflowRuns(projectId ?? '', branch),
    queryFn: async (): Promise<GiteaWorkflowRunsResult> => {
      if (!isTauri() || !projectId) return { runs: [], failedCount: 0 }
      try {
        return await invoke<GiteaWorkflowRunsResult>('list_gitea_workflow_runs', {
          projectId,
          branch: branch ?? null,
        })
      } catch (error) {
        logger.error('Failed to load Gitea workflow runs', { error, projectId })
        throw error
      }
    },
    enabled: (options?.enabled ?? true) && !!projectId,
    staleTime: options?.staleTime ?? 1000 * 60 * 3,
    gcTime: 1000 * 60 * 10,
    retry: 1,
  })
}
