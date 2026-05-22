<script lang="ts">
  import { onDestroy } from 'svelte';
  import {
    ApiAbortError,
    ApiTimeoutError,
    api,
    type OverviewEntry,
    type ReviewSession
  } from '$lib/api';

  const WORKTREE_REF = 'WORKTREE';

  type RepoCandidate = { name: string; path: string };

  let entries: OverviewEntry[] = [];
  let allSessions: ReviewSession[] = [];
  let loading = true;
  let loadError = '';
  let composerOpen = false;

  let homeDir = '';
  let homeDirDraft = '';
  let homeDirEditing = false;
  let homeDirSaving = false;
  let homeDirError = '';
  let homeDirInferred = false;
  let settingsPath: string | null = null;

  let scannedRepos: RepoCandidate[] = [];
  let scanning = false;
  let scanError = '';

  let branches: string[] = [];
  let branchError = '';
  let branchesForPath = '';

  let repoPath = '';
  let repoName = '';
  let baseRef = 'main';
  let headRef = '';
  let title = '';
  let useWorktree = false;
  let creating = false;
  let errorMessage = '';

  $: effectiveHeadRef = useWorktree ? WORKTREE_REF : headRef;
  $: defaultTitle = useWorktree
    ? `${baseRef} → uncommitted`
    : `${baseRef}..${effectiveHeadRef || '?'}`;

  $: byRepo = groupByRepo(entries);
  $: archivedSessions = computeArchived(allSessions, entries);
  $: if (composerOpen) {
    syncRepoSelection(repoName);
  }
  $: if (composerOpen && repoPath) {
    loadBranches(repoPath);
  }

  function groupByRepo(items: OverviewEntry[]): Map<string, OverviewEntry[]> {
    const out = new Map<string, OverviewEntry[]>();
    for (const entry of items) {
      const list = out.get(entry.repo.id) ?? [];
      list.push(entry);
      out.set(entry.repo.id, list);
    }
    return out;
  }

  function computeArchived(
    sessions: ReviewSession[],
    activeEntries: OverviewEntry[]
  ): ReviewSession[] {
    const activeIds = new Set(activeEntries.map((entry) => entry.session.id));
    return sessions.filter((session) => !activeIds.has(session.id));
  }

  let pageController: AbortController | null = null;
  let scanController: AbortController | null = null;
  let loadGeneration = 0;
  let scanGeneration = 0;

  function ignorableError(err: unknown): boolean {
    return err instanceof ApiAbortError || err instanceof ApiTimeoutError;
  }

  async function load() {
    pageController?.abort();
    pageController = new AbortController();
    const signal = pageController.signal;
    const generation = ++loadGeneration;
    loading = true;
    loadError = '';
    try {
      const [overviewData, sessionsData, homeData] = await Promise.all([
        api<{ entries: OverviewEntry[] }>('/api/overview', { signal, timeoutMs: 15_000 }),
        api<{ review_sessions: ReviewSession[] }>('/api/review-sessions', {
          signal,
          timeoutMs: 15_000
        }),
        api<{
          path: string | null;
          inferred: boolean;
          settings_path: string | null;
        }>('/api/settings/home-dir', { signal, timeoutMs: 10_000 }).catch((err) => {
          if (ignorableError(err)) throw err;
          return { path: null, inferred: false, settings_path: null };
        })
      ]);
      if (generation !== loadGeneration) return;
      entries = overviewData.entries;
      allSessions = sessionsData.review_sessions;
      homeDir = homeData?.path ?? '';
      homeDirInferred = homeData?.inferred ?? false;
      settingsPath = homeData?.settings_path ?? null;
      homeDirDraft = homeDir;
      if (homeDir) {
        void scanRepos();
      }
    } catch (error) {
      if (generation !== loadGeneration) return;
      if (ignorableError(error)) return;
      loadError = error instanceof Error ? error.message : String(error);
    } finally {
      if (generation === loadGeneration) loading = false;
    }
  }

  onDestroy(() => {
    pageController?.abort();
    scanController?.abort();
  });

  async function saveHomeDir() {
    if (homeDirSaving) return;
    const value = homeDirDraft.trim();
    if (!value) return;
    homeDirSaving = true;
    homeDirError = '';
    try {
      const response = await api<{ path: string; inferred: boolean }>('/api/settings/home-dir', {
        method: 'PUT',
        body: { path: value }
      });
      homeDir = response.path;
      homeDirInferred = response.inferred;
      homeDirDraft = response.path;
      homeDirEditing = false;
      await scanRepos();
    } catch (error) {
      homeDirError = error instanceof Error ? error.message : String(error);
    } finally {
      homeDirSaving = false;
    }
  }

  async function scanRepos() {
    if (!homeDir) return;
    scanController?.abort();
    scanController = new AbortController();
    const signal = scanController.signal;
    const generation = ++scanGeneration;
    scanning = true;
    scanError = '';
    try {
      const data = await api<{ repos: RepoCandidate[] }>(
        `/api/repo-scan?path=${encodeURIComponent(homeDir)}`,
        { signal, timeoutMs: 20_000 }
      );
      if (generation !== scanGeneration) return;
      scannedRepos = data.repos;
    } catch (error) {
      if (generation !== scanGeneration) return;
      if (ignorableError(error)) {
        scanError = error instanceof ApiTimeoutError ? 'Scan timed out.' : '';
        scannedRepos = [];
        return;
      }
      scanError = error instanceof Error ? error.message : String(error);
      scannedRepos = [];
    } finally {
      if (generation === scanGeneration) scanning = false;
    }
  }

  function syncRepoSelection(name: string) {
    const match = scannedRepos.find((repo) => repo.name === name);
    if (match) {
      repoPath = match.path;
    }
  }

  async function loadBranches(path: string) {
    if (branchesForPath === path) return;
    branchesForPath = path;
    branchError = '';
    try {
      const data = await api<{ branches: string[]; default_branch: string | null }>(
        `/api/repo-branches?path=${encodeURIComponent(path)}`
      );
      branches = data.branches;
      if (data.default_branch && !baseRef) {
        baseRef = data.default_branch;
      }
    } catch (error) {
      branches = [];
      branchError = error instanceof Error ? error.message : String(error);
    }
  }

  async function createSession() {
    if (creating) return;
    creating = true;
    errorMessage = '';
    try {
      const repoData = await api<{ repo: { id: string } }>('/api/repos', {
        method: 'POST',
        body: { path: repoPath, name: repoName || repoPath.split('/').pop() || 'repo' }
      });
      const sessionData = await api<{ review_session: ReviewSession }>('/api/review-sessions', {
        method: 'POST',
        body: {
          repo_id: repoData.repo.id,
          base_ref: baseRef,
          head_ref: effectiveHeadRef,
          branch: useWorktree ? null : headRef,
          title: title || defaultTitle
        }
      });
      window.location.href = `/review/${sessionData.review_session.id}`;
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
      creating = false;
    }
  }

  function formatRelative(timestamp: string): string {
    const then = Date.parse(timestamp);
    if (Number.isNaN(then)) return timestamp;
    const diff = Date.now() - then;
    const minutes = Math.floor(diff / 60000);
    if (minutes < 1) return 'just now';
    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    return `${days}d ago`;
  }

  function liveAgentCount(entry: OverviewEntry, windowMinutes = 30): number {
    const cutoff = Date.now() - windowMinutes * 60_000;
    return entry.agent_sessions.filter((session) => {
      const t = Date.parse(session.last_activity_at);
      return !Number.isNaN(t) && t >= cutoff && !session.ended_at;
    }).length;
  }

  load();
</script>

<main class="shell">
  <header class="topbar">
    <div class="brand">
      <h1>Wyrd Diff</h1>
      <p>Fleet view — every repo, every active review, every agent on the wire.</p>
    </div>
    <div class="topbar-actions">
      <button class="primary" on:click={() => (composerOpen = !composerOpen)}>
        {composerOpen ? 'Cancel' : '+ New review'}
      </button>
      <button class="ghost" on:click={load} disabled={loading}>
        {loading ? 'Refreshing…' : 'Refresh'}
      </button>
    </div>
  </header>

  {#if composerOpen}
    <section class="composer">
      <h2>Create review session</h2>

      <div class="home-bar">
        <span class="home-label">Repos home</span>
        {#if homeDirEditing}
          <input
            class="home-input"
            bind:value={homeDirDraft}
            placeholder="~/code or /Users/you/code"
            on:keydown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                saveHomeDir();
              }
            }}
          />
          <button
            class="bar-btn primary"
            on:click={saveHomeDir}
            disabled={homeDirSaving || !homeDirDraft.trim()}
          >
            {homeDirSaving ? 'Saving…' : 'Save'}
          </button>
          {#if homeDir}
            <button
              class="bar-btn"
              on:click={() => {
                homeDirEditing = false;
                homeDirDraft = homeDir;
                homeDirError = '';
              }}
              disabled={homeDirSaving}>Cancel</button
            >
          {/if}
        {:else if homeDir}
          <code class="home-value">{homeDir}</code>
          {#if homeDirInferred}
            <span class="badge inferred">detected — not yet saved</span>
            <button class="bar-btn primary" on:click={saveHomeDir} disabled={homeDirSaving}>
              {homeDirSaving ? 'Saving…' : 'Confirm + save'}
            </button>
          {/if}
          <button class="bar-btn" on:click={() => (homeDirEditing = true)}>Edit</button>
          <button class="bar-btn" on:click={scanRepos} disabled={scanning}>
            {scanning ? 'Scanning…' : 'Rescan'}
          </button>
          <span class="home-count">
            {scannedRepos.length} repo{scannedRepos.length === 1 ? '' : 's'}
          </span>
        {:else}
          <input
            class="home-input"
            bind:value={homeDirDraft}
            placeholder="~/code or /Users/you/code"
            on:keydown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                saveHomeDir();
              }
            }}
          />
          <button
            class="bar-btn primary"
            on:click={saveHomeDir}
            disabled={homeDirSaving || !homeDirDraft.trim()}
          >
            {homeDirSaving ? 'Saving…' : 'Save'}
          </button>
        {/if}
      </div>
      {#if settingsPath && !homeDirEditing}
        <p class="home-foot">Saved to <code>{settingsPath}</code></p>
      {/if}
      {#if homeDirError}
        <div class="error" role="alert">
          <strong>Could not save home</strong>
          <pre>{homeDirError}</pre>
        </div>
      {/if}
      {#if scanError}
        <div class="error" role="alert">
          <strong>Scan failed</strong>
          <pre>{scanError}</pre>
        </div>
      {/if}

      <div class="composer-grid">
        <label>
          Repo name
          <input
            list="repo-options"
            bind:value={repoName}
            placeholder={scannedRepos.length > 0 ? 'select or type' : 'project name'}
            on:change={() => syncRepoSelection(repoName)}
          />
          <datalist id="repo-options">
            {#each scannedRepos as repo (repo.path)}
              <option value={repo.name}>{repo.path}</option>
            {/each}
          </datalist>
        </label>
        <label>
          Repo path
          <input bind:value={repoPath} placeholder="/Users/you/code/project" />
        </label>
        <label>
          Base ref
          <input list="branch-options" bind:value={baseRef} />
        </label>
        <label>
          Head ref / branch
          <input
            list="branch-options"
            bind:value={headRef}
            disabled={useWorktree}
            placeholder="feature/x"
          />
        </label>
        <datalist id="branch-options">
          {#each branches as branch (branch)}
            <option value={branch}></option>
          {/each}
        </datalist>
        <label class="span-2">
          Title
          <input bind:value={title} placeholder={defaultTitle} />
        </label>
        <label class="checkbox span-2">
          <input type="checkbox" bind:checked={useWorktree} />
          <span>
            Compare working tree to base
            <small>Includes staged + unstaged tracked changes.</small>
          </span>
        </label>
      </div>
      {#if branchError}
        <p class="composer-hint warn">Couldn't list branches for that path: {branchError}</p>
      {:else if repoPath && branches.length > 0}
        <p class="composer-hint">{branches.length} branches available.</p>
      {/if}
      <div class="composer-actions">
        <button class="primary" on:click={createSession} disabled={creating || !repoPath}>
          {creating ? 'Creating…' : 'Create and open'}
        </button>
        {#if errorMessage}
          <div class="error" role="alert">
            <strong>Failed</strong>
            <pre>{errorMessage}</pre>
          </div>
        {/if}
      </div>
    </section>
  {/if}

  {#if loadError}
    <div class="error" role="alert">
      <strong>API unavailable</strong>
      <pre>{loadError}</pre>
    </div>
  {:else if loading && entries.length === 0}
    <p class="placeholder">Loading fleet…</p>
  {:else if entries.length === 0}
    <section class="empty">
      <h2>No active reviews</h2>
      <p>
        Start one with <kbd>+ New review</kbd>, or register a repo + branch from your agent via
        <code>wyrd_diff.create_review_session</code>.
      </p>
    </section>
  {:else}
    <section class="fleet">
      {#each [...byRepo] as [repoId, repoEntries] (repoId)}
        {@const repo = repoEntries[0].repo}
        <article class="repo-card">
          <header>
            <div>
              <h2>{repo.name}</h2>
              <code>{repo.path}</code>
            </div>
            <span class="repo-meta">
              {repoEntries.length} active branch{repoEntries.length === 1 ? '' : 'es'}
            </span>
          </header>
          <ul class="branch-list">
            {#each repoEntries as entry (entry.session.id)}
              {@const live = liveAgentCount(entry)}
              <li>
                <a class="branch-row" href={`/review/${entry.session.id}`}>
                  <div class="branch-head">
                    <span class="branch-name">
                      {entry.branch}
                      {#if entry.session.head_ref === WORKTREE_REF}
                        <span class="badge uncommitted">working tree</span>
                      {/if}
                    </span>
                    <span class="branch-title">{entry.session.title}</span>
                  </div>
                  <div class="branch-stats">
                    <span class="stat" class:warn={entry.pending_thread_count > 0}>
                      <strong>{entry.pending_thread_count}</strong>
                      <small>pending</small>
                    </span>
                    <span class="stat" class:alert={entry.agent_reply_thread_count > 0}>
                      <strong>{entry.agent_reply_thread_count}</strong>
                      <small>awaiting you</small>
                    </span>
                    <span class="stat">
                      <strong>{entry.open_thread_count}</strong>
                      <small>open</small>
                    </span>
                    <span class="stat" class:live={live > 0}>
                      <strong>{live}</strong>
                      <small>live agents</small>
                    </span>
                    <span class="stat muted">
                      <strong>{formatRelative(entry.updated_at)}</strong>
                      <small>updated</small>
                    </span>
                  </div>
                </a>
                {#if entry.agent_sessions.length > 0}
                  <ul class="agent-row">
                    {#each entry.agent_sessions.slice(0, 5) as agent (agent.id)}
                      <li class="agent-chip" class:ended={agent.ended_at}>
                        <span class="dot" class:active={!agent.ended_at}></span>
                        <strong>{agent.agent_name}</strong>
                        <code>{agent.agent_session_id.slice(0, 8)}</code>
                        <small>{formatRelative(agent.last_activity_at)}</small>
                      </li>
                    {/each}
                    {#if entry.agent_sessions.length > 5}
                      <li class="agent-chip muted">+{entry.agent_sessions.length - 5} more</li>
                    {/if}
                  </ul>
                {/if}
              </li>
            {/each}
          </ul>
        </article>
      {/each}
    </section>
  {/if}

  {#if archivedSessions.length > 0}
    <section class="archive">
      <h2>Archived sessions</h2>
      <ul>
        {#each archivedSessions as session (session.id)}
          <li>
            <a href={`/review/${session.id}`}>
              <span>{session.title}</span>
              <code>{session.base_ref} .. {session.head_ref}</code>
            </a>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</main>

<style>
  .shell {
    max-width: 1280px;
    margin: 0 auto;
    padding: 24px 28px 48px;
  }

  .topbar {
    display: flex;
    justify-content: space-between;
    align-items: end;
    gap: 24px;
    padding-bottom: 18px;
    border-bottom: 1px solid var(--wm-border-strong);
  }

  .brand h1 {
    margin: 0;
    font-size: clamp(32px, 5vw, 52px);
    line-height: 1;
    color: var(--wm-green);
    text-shadow: 0 0 14px rgba(124, 255, 158, 0.4);
  }

  .brand p {
    margin: 6px 0 0;
    color: var(--wm-muted);
    font-family: var(--wm-mono);
    font-size: 13px;
  }

  .topbar-actions {
    display: flex;
    gap: 8px;
  }

  button {
    padding: 8px 14px;
    font: 700 12px/1 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    cursor: pointer;
  }

  button.primary {
    border: 1px solid var(--wm-green);
    background: transparent;
    color: var(--wm-green);
  }

  button.ghost {
    border: 1px solid var(--wm-green);
    background: transparent;
    color: var(--wm-green);
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .composer {
    margin: 18px 0 8px;
    padding: 18px;
    border: 1px solid var(--wm-green);
    background: var(--wm-surface);
    box-shadow: 0 0 22px rgba(124, 255, 158, 0.12);
  }

  .composer h2 {
    margin: 0 0 12px;
    font-size: 14px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .home-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
    padding: 8px 10px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    font-family: var(--wm-mono);
    font-size: 12px;
  }

  .home-label {
    color: var(--wm-muted);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-size: 11px;
  }

  .home-value {
    color: var(--wm-amber);
    font-size: 12px;
  }

  .home-input {
    flex: 1 1 240px;
    padding: 6px 8px;
  }

  .bar-btn {
    padding: 6px 10px;
    border: 1px solid var(--wm-green);
    background: transparent;
    color: var(--wm-green);
    font: 700 11px/1 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    cursor: pointer;
  }

  .bar-btn.primary {
    background: transparent;
  }

  .bar-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .badge.inferred {
    padding: 2px 6px;
    border: 1px solid var(--wm-amber);
    color: var(--wm-amber);
    font: 700 10px/1.2 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .home-foot {
    margin: 4px 0 12px;
    color: var(--wm-muted);
    font: 11px/1.3 var(--wm-mono);
  }

  .home-foot code {
    color: var(--wm-amber);
  }

  .home-count {
    color: var(--wm-muted);
    margin-left: auto;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .composer-hint {
    margin: 8px 0 0;
    color: var(--wm-muted);
    font: 12px/1.3 var(--wm-mono);
  }

  .composer-hint.warn {
    color: var(--wm-amber);
  }

  .composer-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }

  .composer-grid label {
    display: block;
    color: var(--wm-muted);
    font: 700 11px/1.3 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .composer-grid .span-2 {
    grid-column: 1 / -1;
  }

  .composer-grid input {
    display: block;
    width: 100%;
    margin-top: 4px;
    padding: 8px 10px;
  }

  .composer-grid label.checkbox {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-ink);
    text-transform: none;
    letter-spacing: 0;
    font: 13px/1.4 var(--wm-mono);
  }

  .composer-grid label.checkbox input {
    width: auto;
    margin: 2px 0 0;
    padding: 0;
  }

  .composer-grid label.checkbox small {
    display: block;
    margin-top: 3px;
    color: var(--wm-muted);
    font-size: 11px;
  }

  .composer-actions {
    margin-top: 14px;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .placeholder {
    margin-top: 24px;
    color: var(--wm-muted);
    font-family: var(--wm-mono);
  }

  .empty {
    margin-top: 24px;
    padding: 32px;
    border: 1px dashed var(--wm-border-strong);
    text-align: center;
  }

  .empty h2 {
    margin: 0 0 8px;
    font-size: 16px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .empty p {
    margin: 0;
    color: var(--wm-muted);
    font-family: var(--wm-mono);
  }

  .empty code,
  .empty kbd {
    padding: 1px 6px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-amber);
    font-family: var(--wm-mono);
  }

  .fleet {
    display: grid;
    gap: 16px;
    margin-top: 18px;
  }

  .repo-card {
    border: 1px solid var(--wm-border-strong);
    background: var(--wm-surface);
  }

  .repo-card > header {
    display: flex;
    justify-content: space-between;
    align-items: end;
    gap: 16px;
    padding: 14px 18px;
    border-bottom: 1px solid var(--wm-border);
  }

  .repo-card h2 {
    margin: 0;
    font-size: 16px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--wm-amber);
  }

  .repo-card code {
    color: var(--wm-muted);
    font-size: 12px;
  }

  .repo-meta {
    color: var(--wm-muted);
    font: 700 11px/1 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .branch-list {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .branch-list > li {
    border-top: 1px solid var(--wm-border);
  }

  .branch-list > li:first-child {
    border-top: none;
  }

  .branch-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 12px;
    padding: 14px 18px;
    color: var(--wm-ink);
    text-decoration: none;
  }

  .branch-row:hover {
    background: rgba(124, 255, 158, 0.04);
  }

  .branch-head {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .branch-name {
    font: 700 14px/1.2 var(--wm-mono);
    color: var(--wm-green);
  }

  .branch-title {
    color: var(--wm-muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .branch-stats {
    display: flex;
    gap: 16px;
    align-items: center;
  }

  .stat {
    display: grid;
    text-align: right;
    line-height: 1.1;
  }

  .stat strong {
    font: 700 16px/1 var(--wm-mono);
    color: var(--wm-ink);
  }

  .stat small {
    color: var(--wm-muted);
    font: 11px/1 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .stat.warn strong {
    color: var(--wm-amber);
    text-shadow: 0 0 8px rgba(255, 191, 0, 0.4);
  }

  .stat.alert strong {
    color: var(--wm-red);
    text-shadow: 0 0 8px rgba(255, 90, 60, 0.45);
  }

  .stat.live strong {
    color: var(--wm-green);
    text-shadow: 0 0 8px rgba(124, 255, 158, 0.5);
  }

  .stat.muted strong {
    color: var(--wm-muted);
    font-size: 12px;
  }

  .agent-row {
    list-style: none;
    margin: 0;
    padding: 0 18px 14px;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .agent-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    font: 12px/1 var(--wm-mono);
  }

  .agent-chip.ended {
    opacity: 0.5;
  }

  .agent-chip.muted {
    color: var(--wm-muted);
  }

  .agent-chip code {
    color: var(--wm-amber);
  }

  .agent-chip small {
    color: var(--wm-muted);
    font-size: 11px;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--wm-muted);
  }

  .dot.active {
    background: var(--wm-green);
    box-shadow: 0 0 6px rgba(124, 255, 158, 0.7);
  }

  .badge.uncommitted {
    margin-left: 8px;
    padding: 1px 6px;
    border: 1px solid var(--wm-amber);
    color: var(--wm-amber);
    font: 700 9px/1.2 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .archive {
    margin-top: 32px;
    padding-top: 18px;
    border-top: 1px solid var(--wm-border);
  }

  .archive h2 {
    margin: 0 0 10px;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--wm-muted);
  }

  .archive ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }

  .archive a {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 10px;
    border: 1px solid var(--wm-border);
    color: var(--wm-ink);
    text-decoration: none;
    font-family: var(--wm-mono);
    font-size: 12px;
  }

  .archive a:hover {
    border-color: var(--wm-amber);
  }

  .archive code {
    color: var(--wm-muted);
  }

  .error {
    margin-top: 12px;
    padding: 10px 12px;
    border: 1px solid var(--wm-red, #ff6b6b);
    background: rgba(255, 107, 107, 0.08);
    color: var(--wm-ink);
  }

  .error strong {
    color: var(--wm-red, #ff6b6b);
    font: 700 12px/1 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .error pre {
    margin: 6px 0 0;
    padding: 6px 8px;
    background: var(--wm-bg);
    border: 1px solid var(--wm-border);
    font: 12px/1.4 var(--wm-mono);
    color: var(--wm-ink);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  input {
    background: var(--wm-bg);
    color: var(--wm-ink);
    border: 1px solid var(--wm-border);
    font-family: var(--wm-mono);
  }

  @media (max-width: 880px) {
    .branch-row {
      grid-template-columns: 1fr;
    }
    .branch-stats {
      justify-content: space-between;
    }
    .composer-grid {
      grid-template-columns: 1fr;
    }
    .composer-grid .span-2 {
      grid-column: auto;
    }
  }
</style>
