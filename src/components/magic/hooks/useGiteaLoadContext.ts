import { useCallback, useMemo, useState } from 'react'
import { toast } from 'sonner'
import { useDebouncedValue } from '@/hooks/useDebouncedValue'
import {
  useGiteaIssues,
  useGiteaPRs,
  useSearchGiteaIssues,
  useSearchGiteaPRs,
  useGetGiteaIssueByNumber,
  useGetGiteaPRByNumber,
  useLoadedGiteaIssueContexts,
  useLoadedGiteaPRContexts,
  filterGiteaIssues,
  filterGiteaPRs,
  mergeWithSearchResults,
  prependExactMatch,
  parseItemNumber,
  loadGiteaIssueContext,
  removeGiteaIssueContext,
  getGiteaIssueContextContent,
  loadGiteaPRContext,
  removeGiteaPRContext,
  getGiteaPRContextContent,
} from '@/services/gitea'
import type { GiteaIssue, GiteaPullRequest } from '@/types/gitea'
import type { ViewingContext } from './useLoadContextHandlers'

interface UseGiteaLoadContextOptions {
  open: boolean
  projectId: string | null
  activeSessionId: string | null
  worktreeId: string | null
  searchQuery: string
  includeClosed: boolean
  setViewingContext: (ctx: ViewingContext | null) => void
  onClearSearch: () => void
}

export function useGiteaLoadContext({
  open,
  projectId,
  activeSessionId,
  worktreeId,
  searchQuery,
  includeClosed,
  setViewingContext,
  onClearSearch,
}: UseGiteaLoadContextOptions) {
  const [loadingGiteaNumbers, setLoadingGiteaNumbers] = useState<Set<number>>(
    new Set()
  )
  const [removingGiteaNumbers, setRemovingGiteaNumbers] = useState<
    Set<number>
  >(new Set())

  const resetGiteaState = useCallback(() => {
    setLoadingGiteaNumbers(new Set())
    setRemovingGiteaNumbers(new Set())
  }, [])

  // Loaded contexts for this session
  const {
    data: loadedGiteaIssueContexts,
    isLoading: isLoadingGiteaIssueContexts,
    refetch: refetchGiteaIssueContexts,
  } = useLoadedGiteaIssueContexts(activeSessionId, worktreeId)

  const {
    data: loadedGiteaPRContexts,
    isLoading: isLoadingGiteaPRContexts,
    refetch: refetchGiteaPRContexts,
  } = useLoadedGiteaPRContexts(activeSessionId, worktreeId)

  // Issue/PR list queries
  const giteaIssueState = includeClosed ? 'all' : 'open'
  const {
    data: giteaIssues,
    isLoading: isLoadingGiteaIssues,
    isFetching: isRefetchingGiteaIssues,
    error: giteaIssuesError,
    refetch: refetchGiteaIssues,
  } = useGiteaIssues(projectId, giteaIssueState, { enabled: open })

  const giteaPRState = includeClosed ? 'all' : 'open'
  const {
    data: giteaPRs,
    isLoading: isLoadingGiteaPRs,
    isFetching: isRefetchingGiteaPRs,
    error: giteaPRsError,
    refetch: refetchGiteaPRs,
  } = useGiteaPRs(projectId, giteaPRState, { enabled: open })

  // Search
  const debouncedSearchQuery = useDebouncedValue(searchQuery, 300)

  const { data: searchedGiteaIssues, isFetching: isSearchingGiteaIssues } =
    useSearchGiteaIssues(projectId, debouncedSearchQuery)
  const { data: searchedGiteaPRs, isFetching: isSearchingGiteaPRs } =
    useSearchGiteaPRs(projectId, debouncedSearchQuery)
  const { data: exactGiteaIssue } = useGetGiteaIssueByNumber(
    projectId,
    debouncedSearchQuery
  )
  const { data: exactGiteaPR } = useGetGiteaPRByNumber(
    projectId,
    debouncedSearchQuery
  )

  const filteredGiteaIssues = useMemo(() => {
    const loadedNumbers = new Set(
      loadedGiteaIssueContexts?.map(c => c.number) ?? []
    )
    if (parseItemNumber(searchQuery) !== null) {
      return exactGiteaIssue && !loadedNumbers.has(exactGiteaIssue.number)
        ? [exactGiteaIssue]
        : []
    }
    const localFiltered = filterGiteaIssues(giteaIssues ?? [], searchQuery)
    const merged = mergeWithSearchResults(localFiltered, searchedGiteaIssues)
    const withExact = prependExactMatch(merged, exactGiteaIssue)
    return withExact.filter(issue => !loadedNumbers.has(issue.number))
  }, [
    giteaIssues,
    searchQuery,
    searchedGiteaIssues,
    loadedGiteaIssueContexts,
    exactGiteaIssue,
  ])

  const filteredGiteaPRs = useMemo(() => {
    const loadedNumbers = new Set(
      loadedGiteaPRContexts?.map(c => c.number) ?? []
    )
    if (parseItemNumber(searchQuery) !== null) {
      return exactGiteaPR && !loadedNumbers.has(exactGiteaPR.number)
        ? [exactGiteaPR]
        : []
    }
    const localFiltered = filterGiteaPRs(giteaPRs ?? [], searchQuery)
    const merged = mergeWithSearchResults(localFiltered, searchedGiteaPRs)
    const withExact = prependExactMatch(merged, exactGiteaPR)
    return withExact.filter(pr => !loadedNumbers.has(pr.number))
  }, [
    giteaPRs,
    searchQuery,
    searchedGiteaPRs,
    loadedGiteaPRContexts,
    exactGiteaPR,
  ])

  // Issue handlers
  const handleLoadGiteaIssue = useCallback(
    async (issueNumber: number, isRefresh = false) => {
      if (!activeSessionId || !projectId) {
        toast.error('No active session')
        return
      }
      setLoadingGiteaNumbers(prev => new Set(prev).add(issueNumber))
      const toastId = toast.loading(
        isRefresh
          ? `Refreshing issue #${issueNumber}...`
          : `Loading issue #${issueNumber}...`
      )
      try {
        const result = await loadGiteaIssueContext(
          activeSessionId,
          issueNumber,
          projectId
        )
        await refetchGiteaIssueContexts()
        toast.success(
          `Issue #${result.number}: ${result.title}${result.commentCount > 0 ? ` (${result.commentCount} comments)` : ''}`,
          { id: toastId }
        )
      } catch (error) {
        toast.error(`${error}`, { id: toastId })
      } finally {
        setLoadingGiteaNumbers(prev => {
          const next = new Set(prev)
          next.delete(issueNumber)
          return next
        })
      }
    },
    [activeSessionId, projectId, refetchGiteaIssueContexts]
  )

  const handleRemoveGiteaIssue = useCallback(
    async (issueNumber: number) => {
      if (!activeSessionId || !projectId) return
      setRemovingGiteaNumbers(prev => new Set(prev).add(issueNumber))
      try {
        await removeGiteaIssueContext(activeSessionId, issueNumber, projectId)
        await refetchGiteaIssueContexts()
        toast.success(`Removed issue #${issueNumber} from context`)
      } catch (error) {
        toast.error(`Failed to remove issue: ${error}`)
      } finally {
        setRemovingGiteaNumbers(prev => {
          const next = new Set(prev)
          next.delete(issueNumber)
          return next
        })
      }
    },
    [activeSessionId, projectId, refetchGiteaIssueContexts]
  )

  const handleViewGiteaIssue = useCallback(
    async (ctx: { number: number; title: string }) => {
      if (!activeSessionId || !projectId) return
      try {
        const content = await getGiteaIssueContextContent(
          activeSessionId,
          ctx.number,
          projectId
        )
        setViewingContext({
          type: 'gitea-issue',
          number: ctx.number,
          title: ctx.title,
          content,
        })
      } catch (error) {
        toast.error(`Failed to load context: ${error}`)
      }
    },
    [activeSessionId, projectId, setViewingContext]
  )

  const handleSelectGiteaIssue = useCallback(
    (issue: GiteaIssue) => {
      handleLoadGiteaIssue(issue.number, false)
      onClearSearch()
    },
    [handleLoadGiteaIssue, onClearSearch]
  )

  // PR handlers
  const handleLoadGiteaPR = useCallback(
    async (prNumber: number, isRefresh = false) => {
      if (!activeSessionId || !projectId) {
        toast.error('No active session')
        return
      }
      setLoadingGiteaNumbers(prev => new Set(prev).add(prNumber))
      const toastId = toast.loading(
        isRefresh ? `Refreshing PR #${prNumber}...` : `Loading PR #${prNumber}...`
      )
      try {
        const result = await loadGiteaPRContext(
          activeSessionId,
          prNumber,
          projectId
        )
        await refetchGiteaPRContexts()
        toast.success(
          `PR #${result.number}: ${result.title}${result.commentCount > 0 ? ` (${result.commentCount} comments)` : ''}${result.reviewCount > 0 ? `, ${result.reviewCount} reviews` : ''}`,
          { id: toastId }
        )
      } catch (error) {
        toast.error(`${error}`, { id: toastId })
      } finally {
        setLoadingGiteaNumbers(prev => {
          const next = new Set(prev)
          next.delete(prNumber)
          return next
        })
      }
    },
    [activeSessionId, projectId, refetchGiteaPRContexts]
  )

  const handleRemoveGiteaPR = useCallback(
    async (prNumber: number) => {
      if (!activeSessionId || !projectId) return
      setRemovingGiteaNumbers(prev => new Set(prev).add(prNumber))
      try {
        await removeGiteaPRContext(activeSessionId, prNumber, projectId)
        await refetchGiteaPRContexts()
        toast.success(`Removed PR #${prNumber} from context`)
      } catch (error) {
        toast.error(`Failed to remove PR: ${error}`)
      } finally {
        setRemovingGiteaNumbers(prev => {
          const next = new Set(prev)
          next.delete(prNumber)
          return next
        })
      }
    },
    [activeSessionId, projectId, refetchGiteaPRContexts]
  )

  const handleViewGiteaPR = useCallback(
    async (ctx: { number: number; title: string }) => {
      if (!activeSessionId || !projectId) return
      try {
        const content = await getGiteaPRContextContent(
          activeSessionId,
          ctx.number,
          projectId
        )
        setViewingContext({
          type: 'gitea-pr',
          number: ctx.number,
          title: ctx.title,
          content,
        })
      } catch (error) {
        toast.error(`Failed to load context: ${error}`)
      }
    },
    [activeSessionId, projectId, setViewingContext]
  )

  const handleSelectGiteaPR = useCallback(
    (pr: GiteaPullRequest) => {
      handleLoadGiteaPR(pr.number, false)
      onClearSearch()
    },
    [handleLoadGiteaPR, onClearSearch]
  )

  return {
    loadedGiteaIssueContexts,
    isLoadingGiteaIssueContexts,
    refetchGiteaIssueContexts,
    loadedGiteaPRContexts,
    isLoadingGiteaPRContexts,
    refetchGiteaPRContexts,

    isLoadingGiteaIssues,
    isRefetchingGiteaIssues,
    isSearchingGiteaIssues,
    giteaIssuesError,
    refetchGiteaIssues,
    isLoadingGiteaPRs,
    isRefetchingGiteaPRs,
    isSearchingGiteaPRs,
    giteaPRsError,
    refetchGiteaPRs,

    filteredGiteaIssues,
    filteredGiteaPRs,

    loadingGiteaNumbers,
    removingGiteaNumbers,
    resetGiteaState,

    handleLoadGiteaIssue,
    handleRemoveGiteaIssue,
    handleViewGiteaIssue,
    handleSelectGiteaIssue,

    handleLoadGiteaPR,
    handleRemoveGiteaPR,
    handleViewGiteaPR,
    handleSelectGiteaPR,

    hasLoadedGiteaIssueContexts: (loadedGiteaIssueContexts?.length ?? 0) > 0,
    hasLoadedGiteaPRContexts: (loadedGiteaPRContexts?.length ?? 0) > 0,
  }
}
