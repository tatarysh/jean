import { Search, Loader2, RefreshCw, AlertCircle, CircleDot, GitPullRequest, Settings } from 'lucide-react'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Input } from '@/components/ui/input'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Tooltip,
  TooltipTrigger,
  TooltipContent,
} from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { isGiteaConfigError, isGiteaAuthError } from '@/services/gitea'
import { LoadedIssueItem, LoadedPRItem } from './LoadContextItems'
import type {
  GiteaIssue,
  GiteaPullRequest,
  LoadedGiteaIssueContext,
  LoadedGiteaPullRequestContext,
} from '@/types/gitea'

type GiteaItemsTabConfig =
  | {
      kind: 'gitea-issues'
      loadedContexts: LoadedGiteaIssueContext[]
      filteredItems: GiteaIssue[]
      onSelectItem: (issue: GiteaIssue) => void
      onViewItem: (ctx: LoadedGiteaIssueContext) => void
      onRemoveItem: (num: number) => void
      onLoadItem: (num: number, refresh: boolean) => void
    }
  | {
      kind: 'gitea-prs'
      loadedContexts: LoadedGiteaPullRequestContext[]
      filteredItems: GiteaPullRequest[]
      onSelectItem: (pr: GiteaPullRequest) => void
      onViewItem: (ctx: LoadedGiteaPullRequestContext) => void
      onRemoveItem: (num: number) => void
      onLoadItem: (num: number, refresh: boolean) => void
    }

interface GiteaItemsTabProps {
  config: GiteaItemsTabConfig
  searchQuery: string
  setSearchQuery: (q: string) => void
  includeClosed: boolean
  setIncludeClosed: (v: boolean) => void
  searchInputRef: React.RefObject<HTMLInputElement | null>
  isLoadingContexts: boolean
  isLoading: boolean
  isRefetching: boolean
  isSearching: boolean
  error: Error | null
  onRefresh: () => void
  selectedIndex: number
  setSelectedIndex: (i: number) => void
  loadingNumbers: Set<number>
  removingNumbers: Set<number>
  hasLoadedContexts: boolean
}

function GiteaErrorState({ error, label }: { error: Error; label: string }) {
  if (isGiteaConfigError(error)) {
    return (
      <div className="flex flex-col items-center justify-center py-8 px-4 text-center gap-2">
        <Settings className="h-5 w-5 text-muted-foreground" />
        <span className="text-sm text-muted-foreground">
          Gitea isn't configured for this project yet.
        </span>
        <span className="text-xs text-muted-foreground">
          Add the instance URL, repository, and access token under Project
          Settings → Integrations.
        </span>
      </div>
    )
  }
  if (isGiteaAuthError(error)) {
    return (
      <div className="flex flex-col items-center justify-center py-8 px-4 text-center gap-2">
        <AlertCircle className="h-5 w-5 text-destructive" />
        <span className="text-sm text-muted-foreground">
          Gitea access token is invalid or missing permissions.
        </span>
        <span className="text-xs text-muted-foreground">
          Update it under Project Settings → Integrations.
        </span>
      </div>
    )
  }
  return (
    <div className="flex flex-col items-center justify-center py-8 px-4 text-center">
      <AlertCircle className="h-5 w-5 text-destructive mb-2" />
      <span className="text-sm text-muted-foreground">
        {error.message || `Failed to load ${label}`}
      </span>
    </div>
  )
}

function GiteaIssueItem({
  issue,
  index,
  isSelected,
  isLoading,
  onMouseEnter,
  onClick,
}: {
  issue: GiteaIssue
  index: number
  isSelected: boolean
  isLoading: boolean
  onMouseEnter: () => void
  onClick: () => void
}) {
  return (
    <button
      data-load-item-index={index}
      onMouseEnter={onMouseEnter}
      onClick={onClick}
      disabled={isLoading}
      className={cn(
        'w-full flex items-start gap-3 px-3 py-2 text-left transition-colors',
        'hover:bg-accent focus:outline-none',
        isSelected && 'bg-accent',
        isLoading && 'opacity-50 cursor-not-allowed'
      )}
    >
      {isLoading ? (
        <Loader2 className="h-4 w-4 mt-0.5 animate-spin text-muted-foreground flex-shrink-0" />
      ) : (
        <CircleDot
          className={cn(
            'h-4 w-4 mt-0.5 flex-shrink-0',
            issue.state === 'open' ? 'text-green-500' : 'text-purple-500'
          )}
        />
      )}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground">#{issue.number}</span>
          <span className="text-sm font-medium truncate">{issue.title}</span>
        </div>
        {issue.labels.length > 0 && (
          <div className="flex flex-wrap gap-1 mt-1">
            {issue.labels.slice(0, 3).map(label => (
              <span
                key={label.name}
                className="px-1.5 py-0.5 text-xs rounded-full"
                style={{
                  backgroundColor: `#${label.color}20`,
                  color: `#${label.color}`,
                  border: `1px solid #${label.color}40`,
                }}
              >
                {label.name}
              </span>
            ))}
            {issue.labels.length > 3 && (
              <span className="text-xs text-muted-foreground">
                +{issue.labels.length - 3}
              </span>
            )}
          </div>
        )}
      </div>
    </button>
  )
}

function GiteaPRItem({
  pr,
  index,
  isSelected,
  isLoading,
  onMouseEnter,
  onClick,
}: {
  pr: GiteaPullRequest
  index: number
  isSelected: boolean
  isLoading: boolean
  onMouseEnter: () => void
  onClick: () => void
}) {
  return (
    <button
      data-load-item-index={index}
      onMouseEnter={onMouseEnter}
      onClick={onClick}
      disabled={isLoading}
      className={cn(
        'w-full flex items-start gap-3 px-3 py-2 text-left transition-colors',
        'hover:bg-accent focus:outline-none',
        isSelected && 'bg-accent',
        isLoading && 'opacity-50 cursor-not-allowed'
      )}
    >
      {isLoading ? (
        <Loader2 className="h-4 w-4 mt-0.5 animate-spin text-muted-foreground flex-shrink-0" />
      ) : (
        <GitPullRequest
          className={cn(
            'h-4 w-4 mt-0.5 flex-shrink-0',
            pr.state === 'open' ? 'text-green-500' : 'text-red-500'
          )}
        />
      )}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground">#{pr.number}</span>
          <span className="text-sm font-medium truncate">{pr.title}</span>
          {pr.isDraft && (
            <span className="text-xs text-muted-foreground bg-muted px-1.5 py-0.5 rounded">
              Draft
            </span>
          )}
        </div>
        <div className="flex items-center gap-2 mt-0.5">
          <span className="text-xs text-muted-foreground truncate">
            {pr.headRefName} → {pr.baseRefName}
          </span>
        </div>
        {pr.labels.length > 0 && (
          <div className="flex flex-wrap gap-1 mt-1">
            {pr.labels.slice(0, 3).map(label => (
              <span
                key={label.name}
                className="px-1.5 py-0.5 text-xs rounded-full"
                style={{
                  backgroundColor: `#${label.color}20`,
                  color: `#${label.color}`,
                  border: `1px solid #${label.color}40`,
                }}
              >
                {label.name}
              </span>
            ))}
            {pr.labels.length > 3 && (
              <span className="text-xs text-muted-foreground">
                +{pr.labels.length - 3}
              </span>
            )}
          </div>
        )}
      </div>
    </button>
  )
}

export function GiteaItemsTab({
  config,
  searchQuery,
  setSearchQuery,
  includeClosed,
  setIncludeClosed,
  searchInputRef,
  isLoadingContexts,
  isLoading,
  isRefetching,
  isSearching,
  error,
  onRefresh,
  selectedIndex,
  setSelectedIndex,
  loadingNumbers,
  removingNumbers,
  hasLoadedContexts,
}: GiteaItemsTabProps) {
  const isIssues = config.kind === 'gitea-issues'
  const label = isIssues ? 'issues' : 'pull requests'
  const searchPlaceholder = isIssues
    ? 'Search Gitea issues by #number, title, or description...'
    : 'Search Gitea PRs by #number, title, or description...'
  const closedLabel = isIssues
    ? 'Include closed issues'
    : 'Include closed/merged PRs'
  const loadedLabel = isIssues ? 'Loaded Issues' : 'Loaded Pull Requests'
  const checkboxId = isIssues
    ? 'load-include-closed-gitea-issues'
    : 'load-include-closed-gitea-prs'

  return (
    <div className="flex flex-col flex-1 min-h-0">
      {isLoadingContexts ? (
        <div className="px-4 py-3 border-b border-border">
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            Loading...
          </div>
        </div>
      ) : hasLoadedContexts ? (
        <div className="border-b border-border">
          <div className="px-4 py-2 text-xs font-medium text-muted-foreground bg-muted/30">
            {loadedLabel}
          </div>
          <div className="max-h-[150px] overflow-y-auto">
            {config.kind === 'gitea-issues'
              ? config.loadedContexts.map(ctx => (
                  <LoadedIssueItem
                    key={ctx.number}
                    context={ctx}
                    isLoading={loadingNumbers.has(ctx.number)}
                    isRemoving={removingNumbers.has(ctx.number)}
                    onRefresh={() => config.onLoadItem(ctx.number, true)}
                    onRemove={() => config.onRemoveItem(ctx.number)}
                    onView={() => config.onViewItem(ctx)}
                  />
                ))
              : config.loadedContexts.map(ctx => (
                  <LoadedPRItem
                    key={ctx.number}
                    context={ctx}
                    isLoading={loadingNumbers.has(ctx.number)}
                    isRemoving={removingNumbers.has(ctx.number)}
                    onRefresh={() => config.onLoadItem(ctx.number, true)}
                    onRemove={() => config.onRemoveItem(ctx.number)}
                    onView={() => config.onViewItem(ctx)}
                  />
                ))}
          </div>
        </div>
      ) : null}

      <div className="p-3 space-y-2 border-b border-border">
        <div className="flex gap-2">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input
              ref={searchInputRef}
              type="text"
              placeholder={searchPlaceholder}
              value={searchQuery}
              onChange={e => setSearchQuery(e.target.value)}
              className="pl-9 h-8 text-base md:text-sm"
            />
          </div>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={onRefresh}
                disabled={isRefetching}
                className={cn(
                  'flex items-center justify-center h-8 w-8 rounded-md border border-border',
                  'hover:bg-accent focus:outline-none focus:ring-2 focus:ring-ring',
                  'transition-colors',
                  isRefetching && 'opacity-50 cursor-not-allowed'
                )}
              >
                <RefreshCw
                  className={cn(
                    'h-4 w-4 text-muted-foreground',
                    isRefetching && 'animate-spin'
                  )}
                />
              </button>
            </TooltipTrigger>
            <TooltipContent>Refresh {label}</TooltipContent>
          </Tooltip>
        </div>
        <div className="flex items-center gap-2">
          <Checkbox
            id={checkboxId}
            checked={includeClosed}
            onCheckedChange={checked => setIncludeClosed(checked === true)}
          />
          <label
            htmlFor={checkboxId}
            className="text-xs text-muted-foreground cursor-pointer"
          >
            {closedLabel}
          </label>
        </div>
      </div>

      <ScrollArea className="flex-1 min-h-0">
        {isLoading && (
          <div className="flex items-center justify-center py-8">
            <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
            <span className="ml-2 text-sm text-muted-foreground">
              Loading {label}...
            </span>
          </div>
        )}

        {error && <GiteaErrorState error={error} label={label} />}

        {!isLoading &&
          !error &&
          config.filteredItems.length === 0 &&
          !isSearching && (
            <div className="flex items-center justify-center py-8">
              <span className="text-sm text-muted-foreground">
                {searchQuery
                  ? `No ${label} match your search`
                  : hasLoadedContexts
                    ? `All open ${label} already loaded`
                    : `No open ${label} found`}
              </span>
            </div>
          )}

        {!isLoading &&
          !error &&
          config.filteredItems.length === 0 &&
          isSearching && (
            <div className="flex items-center justify-center py-8">
              <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
              <span className="ml-2 text-sm text-muted-foreground">
                Searching Gitea...
              </span>
            </div>
          )}

        {!isLoading && !error && config.filteredItems.length > 0 && (
          <div className="py-1">
            {config.kind === 'gitea-issues'
              ? config.filteredItems.map((issue, index) => (
                  <GiteaIssueItem
                    key={issue.number}
                    issue={issue}
                    index={index}
                    isSelected={index === selectedIndex}
                    isLoading={loadingNumbers.has(issue.number)}
                    onMouseEnter={() => setSelectedIndex(index)}
                    onClick={() => config.onSelectItem(issue)}
                  />
                ))
              : config.filteredItems.map((pr, index) => (
                  <GiteaPRItem
                    key={pr.number}
                    pr={pr}
                    index={index}
                    isSelected={index === selectedIndex}
                    isLoading={loadingNumbers.has(pr.number)}
                    onMouseEnter={() => setSelectedIndex(index)}
                    onClick={() => config.onSelectItem(pr)}
                  />
                ))}
            {isSearching && (
              <div className="flex items-center justify-center py-2">
                <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />
                <span className="ml-1.5 text-xs text-muted-foreground">
                  Searching Gitea for more results...
                </span>
              </div>
            )}
          </div>
        )}
      </ScrollArea>
    </div>
  )
}
