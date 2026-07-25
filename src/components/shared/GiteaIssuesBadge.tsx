import { useCallback } from 'react'
import { CircleDot } from 'lucide-react'
import { cn } from '@/lib/utils'
import {
  Tooltip,
  TooltipTrigger,
  TooltipContent,
} from '@/components/ui/tooltip'
import { useGiteaIssues } from '@/services/gitea'
import { useProjectsStore } from '@/store/projects-store'

const BADGE_STALE_TIME = 5 * 60 * 1000 // 5 minutes — background badge, not active UI

interface GiteaIssuesBadgeProps {
  projectId: string
  giteaUrl?: string | null
  className?: string
}

export function GiteaIssuesBadge({
  projectId,
  giteaUrl,
  className,
}: GiteaIssuesBadgeProps) {
  const { data: issues } = useGiteaIssues(projectId, 'open', {
    enabled: !!giteaUrl,
    staleTime: BADGE_STALE_TIME,
  })

  const totalCount = issues?.length ?? 0

  const handleClick = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      useProjectsStore.getState().selectProject(projectId)
    },
    [projectId]
  )

  if (!giteaUrl || totalCount === 0) return null

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          onClick={handleClick}
          className={cn(
            'shrink-0 rounded bg-green-500/10 px-1.5 py-0.5 text-[11px] font-medium text-green-600 transition-colors hover:bg-green-500/20',
            className
          )}
        >
          <span className="flex items-center gap-0.5">
            <CircleDot className="h-3 w-3" />
            {totalCount}
          </span>
        </button>
      </TooltipTrigger>
      <TooltipContent>{`${totalCount} open Gitea issue${totalCount > 1 ? 's' : ''}`}</TooltipContent>
    </Tooltip>
  )
}
