import { CircleDot, ExternalLink } from 'lucide-react'
import type { AppCommand } from './types'
import { useUIStore } from '@/store/ui-store'
import { useProjectsStore } from '@/store/projects-store'
import { projectsQueryKeys } from '@/services/projects'
import { openExternal, preOpenWindow } from '@/lib/platform'
import type { Project } from '@/types/projects'

function getSelectedProjectWithGitea(
  queryClient: import('@tanstack/react-query').QueryClient
): Project | undefined {
  const { selectedProjectId } = useProjectsStore.getState()
  if (!selectedProjectId) return undefined
  const projects = queryClient.getQueryData<Project[]>(
    projectsQueryKeys.list()
  )
  const project = projects?.find(p => p.id === selectedProjectId)
  return project?.gitea_url ? project : undefined
}

export const giteaCommands: AppCommand[] = [
  {
    id: 'load-gitea-context',
    label: 'Load Gitea Context',
    description: 'Browse and attach Gitea issues/PRs to the current session',
    icon: CircleDot,
    group: 'gitea',
    keywords: ['gitea', 'issues', 'pull', 'requests', 'pr', 'context'],

    execute: () => {
      useUIStore.getState().setLoadContextModalOpen(true)
    },
    // Load Context is session-scoped (attaches to the active session), unlike
    // the project-scoped New Worktree modal — requires an active session, not
    // just a selected project.
    isAvailable: context =>
      context.hasActiveSession() &&
      !!getSelectedProjectWithGitea(context.queryClient),
  },

  {
    id: 'open-gitea-repository',
    label: 'Open Gitea Repository',
    description: "Open this project's Gitea repository in the browser",
    icon: ExternalLink,
    group: 'gitea',
    keywords: ['gitea', 'repository', 'browser', 'open'],

    execute: async context => {
      const project = getSelectedProjectWithGitea(context.queryClient)
      if (!project?.gitea_url || !project.gitea_owner || !project.gitea_repo)
        return
      const preOpened = preOpenWindow()
      await openExternal(
        `${project.gitea_url.replace(/\/$/, '')}/${project.gitea_owner}/${project.gitea_repo}`,
        preOpened
      )
    },
    isAvailable: context =>
      context.hasSelectedProject() &&
      !!getSelectedProjectWithGitea(context.queryClient),
  },
]
