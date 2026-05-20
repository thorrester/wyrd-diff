<script lang="ts">
  import { api, type ReviewSession } from '$lib/api';

  let repoPath = '/Users/stevenforrester/Documents/GitHub/wyrd';
  let repoName = 'wyrd';
  let baseRef = 'phase-2a/skald-scaffolds';
  let headRef = 'phase-2b/skald-providers';
  let title = '';
  let sessions: ReviewSession[] = [];
  let message = '';

  async function loadSessions() {
    const data = await api<{ review_sessions: ReviewSession[] }>('/api/review-sessions');
    sessions = data.review_sessions;
  }

  async function createSession() {
    message = 'Creating session...';
    const repoData = await api<{ repo: { id: string } }>('/api/repos', {
      method: 'POST',
      body: { path: repoPath, name: repoName }
    });
    const sessionData = await api<{ review_session: ReviewSession }>('/api/review-sessions', {
      method: 'POST',
      body: {
        repo_id: repoData.repo.id,
        base_ref: baseRef,
        head_ref: headRef,
        title: title || `${baseRef}..${headRef}`
      }
    });
    window.location.href = `/review/${sessionData.review_session.id}`;
  }

  loadSessions().catch((error) => {
    message = `API unavailable: ${error.message}`;
  });
</script>

<main class="shell">
  <section class="hero">
    <div>
      <p class="eyebrow">local review memory</p>
      <h1>Wyrd Mind</h1>
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
          <input bind:value={headRef} />
        </label>
      </div>
      <button on:click={createSession}>Create and open</button>
      {#if message}<p class="message">{message}</p>{/if}
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
              <span>{session.base_ref} .. {session.head_ref}</span>
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
    border-bottom: 2px solid var(--wm-border-strong);
  }

  h1,
  h2 {
    margin: 0 0 12px;
  }

  h1 {
    font-size: clamp(42px, 8vw, 86px);
    line-height: 0.88;
    letter-spacing: 0;
  }

  h2 {
    font-size: 18px;
    text-transform: uppercase;
  }

  .eyebrow {
    margin: 0 0 8px;
    color: var(--wm-green);
    font: 700 12px/1 var(--wm-mono);
    letter-spacing: 0;
    text-transform: uppercase;
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
    border: 2px solid var(--wm-border-strong);
    background: var(--wm-surface);
    box-shadow: var(--wm-shadow);
  }

  .composer {
    border-color: var(--wm-green);
  }

  label {
    display: block;
    margin: 10px 0;
    color: var(--wm-muted);
    font: 700 12px/1.3 var(--wm-mono);
    text-transform: uppercase;
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
    border: 2px solid var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-ink);
    text-decoration: none;
  }

  .sessions a:hover {
    border-color: var(--wm-amber);
    box-shadow: 3px 3px 0 var(--wm-black);
  }

  .sessions span,
  .message {
    color: var(--wm-amber);
    font-family: var(--wm-mono);
    overflow-wrap: anywhere;
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
