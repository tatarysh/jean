# Gitea Integration — Remaining Work

## Context

Added a self-hosted Gitea integration parallel to the existing GitHub one. Committed in `129fc235` (branch `main`, pushed to `tatarysh/jean` fork — `origin` remote was repointed from `coollabsio/jean` to `tatarysh/jean`).

Scope was deliberately narrowed during implementation after live testing against a real Gitea instance (`gitea.rysh`, test repo `rysh/jean-gitea-test`, PR #2 created for verification). Read this whole file before continuing — it captures decisions that aren't obvious from the code alone.

## Key decisions (why things look the way they do)

- **No `tea` CLI / no bundled binary.** Original plan mirrored `gh_cli/` (bundle+manage a CLI binary). Live testing showed `tea`'s own `issues list`/`pulls list` commands return lossy, inconsistent display-projected data (e.g. `state` as `'OPEN'` vs raw API's `'open'`, author as full name not login, field names differing between list/view). `tea api` (raw REST passthrough) returns clean Swagger-shaped JSON. Since it's just a thin wrapper over a normal HTTP call, the integration calls Gitea's REST API directly via `reqwest`, mirroring the existing **Sentry** integration's pattern (`sentry_issues.rs`), not GitHub's CLI-shelling pattern.
- **No git-remote parsing for owner/repo.** GitHub's `get_repo_identifier()` parses `owner/repo` from the git remote URL. Gitea can't reliably do this: self-hosted instances often have a different SSH host (e.g. an internal Docker IP) than the public HTTPS/API host. Instead, `Project.gitea_owner`/`gitea_repo` are explicit user-set fields, like Sentry's `sentry_organization_slug`/`sentry_project_slug`.
- **Per-project only, no global fallback.** Unlike Sentry (which has a global token + per-project override), Gitea has no global config — different projects may point at entirely different self-hosted instances.
- **Auth errors use HTTP status codes** (401/403/404), not stderr string-matching like GitHub's `is_gh_cli_auth_error`. Simpler and more reliable since we control the HTTP client directly.
- **Dependabot alerts / repository security advisories are explicitly out of scope.** Gitea has no equivalent to the GitHub Advisory Database.
- **Load Context modal integration only** (see below) — the New Worktree modal (creating a branch/worktree directly from an issue/PR) was scoped out.

## What's done

- `jean-core/src/projects/types.rs` — `Project.gitea_url`/`gitea_token`/`gitea_owner`/`gitea_repo`
- `jean-core/src/projects/commands.rs` — `update_project_settings` handles all four (empty-string-clears convention, like `worktrees_dir`)
- `jean-core/src/projects/gitea_issues.rs` — REST client, issue/PR CRUD-ish operations, loaded-context persistence (`gitea-{owner}-{repo}-issue-{n}.md` files, `ContextReferences.gitea_issues`/`gitea_prs` maps — prefixed/separate from GitHub's to avoid collisions)
- `jean-core/src/projects/gitea_actions.rs` — `list_gitea_workflow_runs` via `/repos/{owner}/{repo}/actions/tasks` (confirmed live: returns `{workflow_runs, total_count}`, same envelope as GitHub Actions API)
- `jean-core/src/http_server/dispatch.rs` — 20 new command match arms (list/get issues+PRs, review comments, contexts load/list/remove/get-content, workflow runs, `test_gitea_connection`)
- `src/types/gitea.ts`, `src/services/gitea.ts` — frontend types/hooks (all camelCase, unlike `github.ts` which has some snake_case fields from raw `gh` CLI JSON)
- `src/components/projects/panes/IntegrationsPane.tsx` — "Gitea Integration" section (repo owner/repo, instance URL, token, Test Connection button)
- `src/components/magic/GiteaItemsTab.tsx`, `src/components/magic/hooks/useGiteaLoadContext.ts` — Load Context modal tabs ("Gitea Issues"/"Gitea PRs"), gated on `project.gitea_url` being set. Reuses `LoadedIssueItem`/`LoadedPRItem` from `LoadContextItems.tsx` (structurally compatible); has its own `GiteaIssueItem`/`GiteaPRItem` for search results since GitHub's hardcode `issue.created_at` (snake_case) and `state === 'OPEN'` (uppercase) — Gitea uses `createdAt`/`'open'`.
- `src/lib/commands/gitea-commands.ts` — command palette: "Load Gitea Context", "Open Gitea Repository"
- `src/components/shared/GiteaIssuesBadge.tsx`, `GiteaPRsBadge.tsx` — open issue/PR counts on project rows (`ProjectTreeItem.tsx`, `ProjectCanvasView.tsx`); click currently just selects the project (see gap below)

Backend verified: `cargo check --tests`, `cargo clippy`, `cargo test --lib` all clean (909 tests passing). Frontend **not** typechecked/linted — no `bun`/`node_modules` in the sandbox this was built in.

## What's left

### 1. Verify the build (do this first)

```
bun run typecheck
bun run lint
bun run tauri dev
```

Nothing was visually tested. Manual test path: Project Settings → Integrations → Gitea section (fill in owner/repo/URL/token, hit Test Connection) → open a chat session → Load Context modal → "Gitea Issues"/"Gitea PRs" tabs. Test data is available on `gitea.rysh`, repo `rysh/jean-gitea-test` (has 1 issue, 1 PR — #2 — with a branch `test-pr-branch`).

### 2. Gitea Actions UI (real gap, was in original scope)

Backend (`list_gitea_workflow_runs`) and frontend hook (`useGiteaWorkflowRuns` in `src/services/gitea.ts`) exist but **nothing renders them**. Need a UI entry point — either:

- Generalize `src/components/shared/WorkflowRunsModal.tsx` to accept a `provider: 'github' | 'gitea'` prop (swaps the data hook and the `extractRunId` URL regex — GitHub's is `/runs/(\d+)/`, Gitea's run URLs need checking against a real populated run, e.g. `https://gitea.rysh/rysh/jean-gitea-test/actions/runs/{id}`), or
- A dedicated `GiteaWorkflowRunsModal.tsx`.

Note: the per-run field shape (`GiteaWorkflowRun` in `gitea_actions.rs`) was defensively parsed (skip-on-error per item) because it was only verified against an **empty** `workflow_runs` array on the test repo — never against a populated one. Before building the UI, trigger a real workflow run on `rysh/jean-gitea-test` (or another repo with Actions configured) and re-check the actual field names/shapes coming back from `/repos/{owner}/{repo}/actions/tasks`.

### 3. Load Context modal polish (scoped out for time)

- No keyboard navigation on the Gitea tabs (`useLoadContextKeyboard.ts` wasn't touched — arrow keys / Enter / Cmd+6/Cmd+7 tab-switch don't work, mouse-only for now)
- No "preview before load" (eye icon) — GitHub has `IssuePreviewModal.tsx` for a live preview before attaching; Gitea items just load directly on click

### 4. New Worktree modal — creating a branch/worktree from a Gitea issue/PR (biggest remaining piece)

This is a **different code path** from Load Context — it goes through `jean-core/src/projects/commands.rs::create_worktree`, a ~900-line function handling branch naming, git operations, session creation, background processing, event emission, and context-file writing for **every** provider (GitHub issue/PR, Linear, Sentry, security alert, advisory) in one function. It was deliberately not touched in this pass because:

- It's the single highest-blast-radius function in the codebase (touches worktree creation for everything, not just Gitea)
- No way to test live in the environment this was built in

To add Gitea here: extend `create_worktree`'s signature with `gitea_issue_context: Option<GiteaIssueContext>` / `gitea_pr_context: Option<GiteaPullRequestContext>`, thread through `dispatch.rs`, extend `useCreateWorktree` in `src/services/projects.ts`, then wire `handleSelectGiteaIssue`/`handleSelectGiteaPR` (+ investigate variants) into `src/components/worktree/hooks/useNewWorktreeHandlers.ts`, add Gitea tabs to `NewWorktreeModal.tsx`/`useNewWorktreeData.ts`, and new `GiteaIssuesTab.tsx`/`GiteaPRsTab.tsx` components (the existing `IssueItem`/`PRItem` in `NewWorktreeItems.tsx` can't be reused as-is for the same reason as the Load Context tab — hardcoded `created_at`/`'OPEN'`).

There's a real, separate discussion worth having before doing this: `create_worktree` has visible duplication across its per-provider branches (branch naming, context-markdown writing, auto-investigate marking each repeated ~6 times). Worth considering a shared abstraction (e.g. a `WorktreeContextSource` enum) as its **own** refactor, tested thoroughly against all existing providers, before adding Gitea as a 7th branch — don't do the refactor and the Gitea addition in the same change.

Do this with the app running so each provider's worktree-creation flow (not just Gitea's) can be re-tested after the change.

## Verification checklist for whoever picks this up

- [ ] `bun run typecheck` / `bun run lint` clean
- [ ] Gitea settings save/clear correctly in Project Settings → Integrations
- [ ] Test Connection button succeeds against a real instance
- [ ] Load Context → Gitea Issues/PRs tabs list, search, load, remove, view
- [ ] Badges show correct counts on project rows, disappear when `gitea_url` unset
- [ ] Command palette entries appear/hide correctly based on active session + project config
- [ ] `cargo test --lib` / `cargo clippy` still clean in `jean-core/` after any further changes
