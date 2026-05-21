<script lang="ts">
  import { api, type ReviewSession } from '$lib/api';

  const WORKTREE_REF = 'WORKTREE';

  let repoPath = '/Users/stevenforrester/Documents/GitHub/wyrd';
  let repoName = 'wyrd';
  let baseRef = 'phase-2a/skald-scaffolds';
  let headRef = 'phase-2b/skald-providers';
  let title = '';
  let useWorktree = false;
  let sessions: ReviewSession[] = [];
  let message = '';

  $: effectiveHeadRef = useWorktree ? WORKTREE_REF : headRef;
  $: defaultTitle = useWorktree
    ? `${baseRef} → uncommitted`
    : `${baseRef}..${effectiveHeadRef}`;

  async function loadSessions() {
    const data = await api<{ review_sessions: ReviewSession[] }>('/api/review-sessions');
    sessions = data.review_sessions;
  }

  let creating = false;
  let errorMessage = '';

  async function createSession() {
    if (creating) return;
    creating = true;
    errorMessage = '';
    message = 'Creating session...';
    try {
      const repoData = await api<{ repo: { id: string } }>('/api/repos', {
        method: 'POST',
        body: { path: repoPath, name: repoName }
      });
      const sessionData = await api<{ review_session: ReviewSession }>('/api/review-sessions', {
        method: 'POST',
        body: {
          repo_id: repoData.repo.id,
          base_ref: baseRef,
          head_ref: effectiveHeadRef,
          title: title || defaultTitle
        }
      });
      window.location.href = `/review/${sessionData.review_session.id}`;
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      errorMessage = detail || 'Unknown error';
      message = '';
      creating = false;
    }
  }

  loadSessions().catch((error) => {
    message = `API unavailable: ${error.message}`;
  });
</script>

<main class="shell">
  <section class="hero">
    <div>
      <p class="eyebrow">local review memory</p>
      <h1>Wyrd Diff</h1>
    </div>
    <p>Review diffs, record thinking, expose agent-ready context, and preserve code trajectory.</p>
  </section>

  <div class="workspace">
    <section class="panel composer">
      <h2>Create review session</h2>
      <label>
        Repository path
        <input bind:value={repoPath} />
      </label>
      <div class="row">
        <label>
          Repository name
          <input bind:value={repoName} />
        </label>
        <label>
          Title
          <input bind:value={title} placeholder="Optional" />
        </label>
      </div>
      <div class="row">
        <label>
          Base ref
          <input bind:value={baseRef} />
        </label>
        <label>
          Head ref
          <input bind:value={headRef} disabled={useWorktree} class:disabled={useWorktree} />
        </label>
      </div>
      <label class="checkbox">
        <input type="checkbox" bind:checked={useWorktree} />
        <span>
          Compare working tree to base
          <small>Include staged + unstaged tracked changes. Skips untracked files.</small>
        </span>
      </label>
      <button on:click={createSession} disabled={creating}>
        {creating ? 'Creating…' : 'Create and open'}
      </button>
      {#if errorMessage}
        <div class="error" role="alert">
          <div class="error-head">
            <strong>Failed to create session</strong>
            <button type="button" class="retry" on:click={createSession}>Retry</button>
          </div>
          <pre>{errorMessage}</pre>
          <p class="hint">
            Check that <code>{baseRef}</code> and <code>{effectiveHeadRef}</code> exist in
            <code>{repoPath}</code>. Branches must be resolvable by <code>git rev-parse</code>.
          </p>
        </div>
      {:else if message}
        <p class="message">{message}</p>
      {/if}
    </section>

    <section class="panel">
      <h2>Recent sessions</h2>
      {#if sessions.length === 0}
        <p>No sessions yet.</p>
      {:else}
        <div class="sessions">
          {#each sessions as session (session.id)}
            <a href={`/review/${session.id}`}>
              <strong>{session.title}</strong>
              <span>
                {session.base_ref} ..
                {session.head_ref === 'WORKTREE' ? 'working tree' : session.head_ref}
                {#if session.head_ref === 'WORKTREE'}
                  <em class="uncommitted-tag">uncommitted</em>
                {/if}
              </span>
            </a>
          {/each}
        </div>
      {/if}
    </section>
  </div>
</main>

<style>
  .shell {
    max-width: 1180px;
    margin: 0 auto;
    padding: 28px;
  }

  .hero {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(280px, 420px);
    gap: 24px;
    align-items: end;
    padding: 22px 0 26px;
    border-bottom: 1px solid var(--wm-border-strong);
    box-shadow: 0 1px 0 rgba(124, 255, 158, 0.12);
  }

  h1,
  h2 {
    margin: 0 0 12px;
  }

  h1 {
    font-size: clamp(42px, 8vw, 86px);
    line-height: 0.88;
    letter-spacing: 0.01em;
    color: var(--wm-green);
    text-shadow:
      0 0 18px rgba(124, 255, 158, 0.55),
      0 0 36px rgba(124, 255, 158, 0.25);
  }

  h2 {
    font-size: 18px;
    text-transform: uppercase;
  }

  .eyebrow {
    margin: 0 0 8px;
    color: var(--wm-green);
    font: 700 12px/1 var(--wm-mono);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    text-shadow: 0 0 10px rgba(124, 255, 158, 0.7);
  }

  p {
    color: var(--wm-muted);
  }

  .workspace {
    display: grid;
    grid-template-columns: minmax(0, 1.35fr) minmax(300px, 0.65fr);
    gap: 22px;
    margin-top: 24px;
  }

  .panel {
    padding: 18px;
    border: 1px solid var(--wm-border-strong);
    background: var(--wm-surface);
    box-shadow: var(--wm-shadow);
  }

  .composer {
    border-color: var(--wm-green);
    box-shadow: 0 0 22px rgba(124, 255, 158, 0.14);
  }

  label {
    display: block;
    margin: 10px 0;
    color: var(--wm-muted);
    font: 700 12px/1.3 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .row {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
  }

  input {
    display: block;
    width: 100%;
    margin-top: 4px;
    padding: 10px;
  }

  input.disabled,
  input:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  label.checkbox {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    margin: 14px 0 4px;
    padding: 10px 12px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-ink);
    text-transform: none;
    letter-spacing: 0;
    font: 13px/1.4 var(--wm-mono);
    cursor: pointer;
  }

  label.checkbox input {
    width: auto;
    margin: 3px 0 0;
    padding: 0;
    accent-color: var(--wm-amber);
  }

  label.checkbox small {
    display: block;
    margin-top: 3px;
    color: var(--wm-muted);
    font-size: 11px;
  }

  button {
    margin-top: 8px;
    padding: 8px 12px;
  }

  .sessions {
    display: grid;
    gap: 10px;
  }

  .sessions a {
    display: grid;
    gap: 4px;
    padding: 10px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-ink);
    text-decoration: none;
    transition:
      border-color 0.12s,
      box-shadow 0.12s;
  }

  .sessions a:hover {
    border-color: var(--wm-amber);
    box-shadow: var(--wm-glow-amber);
  }

  .uncommitted-tag {
    margin-left: 6px;
    padding: 1px 5px;
    border: 1px solid var(--wm-amber);
    color: var(--wm-amber);
    font: 700 9px/1.2 var(--wm-mono);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    font-style: normal;
  }

  .sessions span,
  .message {
    color: var(--wm-amber);
    font-family: var(--wm-mono);
    overflow-wrap: anywhere;
  }

  button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .error {
    margin-top: 12px;
    padding: 12px;
    border: 1px solid var(--wm-red, #ff6b6b);
    background: var(--wm-red-bg, rgba(255, 107, 107, 0.08));
    box-shadow: 0 0 14px rgba(255, 107, 107, 0.18);
  }

  .error-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .error-head strong {
    color: var(--wm-red, #ff6b6b);
    font: 700 12px/1 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .error pre {
    margin: 0;
    padding: 8px;
    background: var(--wm-bg);
    border: 1px solid var(--wm-border);
    font: 12px/1.4 var(--wm-mono);
    color: var(--wm-ink);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 180px;
    overflow-y: auto;
  }

  .error .hint {
    margin: 8px 0 0;
    color: var(--wm-muted);
    font-size: 12px;
  }

  .error code {
    padding: 1px 4px;
    background: var(--wm-bg);
    border: 1px solid var(--wm-border);
    font-family: var(--wm-mono);
    color: var(--wm-amber);
  }

  .retry {
    margin: 0;
    padding: 4px 10px;
    font: 700 11px/1 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  @media (max-width: 860px) {
    .shell {
      padding: 16px;
    }

    .hero,
    .workspace,
    .row {
      grid-template-columns: 1fr;
    }
  }
</style>
