<script lang="ts">
  import { page } from '$app/stores';
  import { Background, Controls, MiniMap, SvelteFlow, type Edge, type Node } from '@xyflow/svelte';
  import '@xyflow/svelte/dist/style.css';
  import { api, type AgentContext, type FixImportRecord } from '$lib/api';

  let context: AgentContext | null = null;
  let fixes: FixImportRecord[] = [];
  let message = '';

  $: sessionId = $page.params.id;
  $: nodes = buildNodes(context, fixes);
  $: edges = buildEdges(context, fixes);

  async function load() {
    const contextData = await api<AgentContext>(`/api/review-sessions/${sessionId}/agent-context`);
    const fixesData = await api<{ fix_imports: FixImportRecord[] }>(
      `/api/review-sessions/${sessionId}/fix-imports`
    );
    context = contextData;
    fixes = fixesData.fix_imports;
  }

  function buildNodes(data: AgentContext | null, fixRecords: FixImportRecord[]): Node[] {
    if (!data) return [];
    const commentNodes = data.open_comments.map((comment, index) => ({
      id: `comment-${comment.id}`,
      position: { x: 340, y: 80 + index * 110 },
      data: {
        label: `Comment\n${comment.file_path}:${comment.new_line ?? comment.old_line ?? '?'}\n${truncate(comment.body)}`
      },
      class: 'comment-node'
    }));
    const noteNodes = data.agent_visible_notes.map((note, index) => ({
      id: `note-${note.id}`,
      position: { x: 340, y: 80 + (commentNodes.length + index) * 110 },
      data: {
        label: `Note\n${note.title || 'Untitled'}\n${truncate(note.body)}`
      },
      class: 'note-node'
    }));
    const decisionNodes = data.accepted_decisions.map((decision, index) => ({
      id: `decision-${decision.id}`,
      position: { x: 670, y: 120 + index * 130 },
      data: {
        label: `Decision\n${decision.title || 'Untitled'}\n${truncate(decision.decision)}`
      },
      class: 'decision-node'
    }));
    const fixNodes = fixRecords.map((fix, index) => ({
      id: `fix-${fix.id}`,
      position: { x: 1010, y: 120 + index * 130 },
      data: {
        label: `Fix\n${fix.commit_sha.slice(0, 12)}\n${fix.agent_name ?? 'agent'}${fix.accepted ? ' accepted' : ''}`
      },
      class: 'fix-node'
    }));

    return [
      {
        id: 'session',
        position: { x: 40, y: 180 },
        data: {
          label: `Session\n${data.session.title}\n${data.session.base_ref}..${data.session.head_ref}`
        },
        class: 'session-node'
      },
      ...commentNodes,
      ...noteNodes,
      ...decisionNodes,
      ...fixNodes
    ];
  }

  function buildEdges(data: AgentContext | null, fixRecords: FixImportRecord[]): Edge[] {
    if (!data) return [];
    const evidence = [
      ...data.open_comments.map((comment) => `comment-${comment.id}`),
      ...data.agent_visible_notes.map((note) => `note-${note.id}`)
    ];
    const decisions = data.accepted_decisions.map((decision) => `decision-${decision.id}`);
    const edgesFromSession = evidence.map((id) => ({
      id: `session-${id}`,
      source: 'session',
      target: id,
      animated: true
    }));
    const edgesToDecisions = decisions.flatMap((decisionId) => {
      const sources = evidence.length > 0 ? evidence : ['session'];
      return sources.map((source) => ({
        id: `${source}-${decisionId}`,
        source,
        target: decisionId
      }));
    });
    const fixSources =
      decisions.length > 0 ? decisions : evidence.length > 0 ? evidence : ['session'];
    const edgesToFixes = fixRecords.flatMap((fix) =>
      fixSources.map((source) => ({
        id: `${source}-fix-${fix.id}`,
        source,
        target: `fix-${fix.id}`,
        animated: fix.accepted
      }))
    );
    return [...edgesFromSession, ...edgesToDecisions, ...edgesToFixes];
  }

  function truncate(value: string) {
    return value.length > 82 ? `${value.slice(0, 79)}...` : value;
  }

  load().catch((error) => {
    message = error.message;
  });
</script>

<main class="trajectory">
  <header>
    <a href={`/review/${sessionId}`}>Back to review</a>
    <div>
      <p>Trajectory</p>
      <h1>{context?.session.title ?? 'Review session'}</h1>
    </div>
  </header>

  {#if message}
    <section class="message">{message}</section>
  {:else}
    <section class="graph">
      <SvelteFlow {nodes} {edges} fitView minZoom={0.35} maxZoom={1.4}>
        <Background />
        <MiniMap />
        <Controls />
      </SvelteFlow>
    </section>
  {/if}
</main>

<style>
  :global(body) {
    overflow: hidden;
  }

  .trajectory {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    min-height: 100vh;
    background: var(--wm-bg);
  }

  header {
    display: flex;
    gap: 16px;
    align-items: center;
    padding: 14px;
    border-bottom: 2px solid var(--wm-border-strong);
    background: var(--wm-surface);
  }

  header a {
    padding: 7px 10px;
    border: 2px solid var(--wm-border-strong);
    background: var(--wm-ink);
    color: var(--wm-black);
    box-shadow: 3px 3px 0 var(--wm-black);
    font-size: 12px;
    font-weight: 800;
    text-decoration: none;
  }

  h1,
  p {
    margin: 0;
  }

  p {
    color: var(--wm-green);
    font: 700 11px/1.2 var(--wm-mono);
    text-transform: uppercase;
  }

  h1 {
    font-size: 20px;
  }

  .graph {
    min-height: 0;
  }

  .message {
    margin: 16px;
    padding: 14px;
    border: 2px solid var(--wm-red);
    background: var(--wm-red-bg);
    color: var(--wm-ink);
  }

  :global(.svelte-flow) {
    background:
      linear-gradient(rgba(124, 255, 158, 0.03) 1px, transparent 1px),
      linear-gradient(90deg, rgba(124, 255, 158, 0.025) 1px, transparent 1px), var(--wm-bg);
    background-size: 22px 22px;
  }

  :global(.svelte-flow__node) {
    width: 230px;
    padding: 10px;
    border: 2px solid var(--wm-border-strong);
    background: var(--wm-surface);
    color: var(--wm-ink);
    box-shadow: 4px 4px 0 var(--wm-black);
    font: 12px/1.35 var(--wm-mono);
    white-space: pre-wrap;
  }

  :global(.svelte-flow__node.session-node) {
    border-color: var(--wm-green);
  }

  :global(.svelte-flow__node.comment-node) {
    border-color: var(--wm-amber);
  }

  :global(.svelte-flow__node.note-node) {
    border-color: var(--wm-blue);
  }

  :global(.svelte-flow__node.decision-node) {
    border-color: var(--wm-purple);
  }

  :global(.svelte-flow__node.fix-node) {
    border-color: var(--wm-green);
    background: var(--wm-green-bg);
  }

  :global(.svelte-flow__edge-path) {
    stroke: var(--wm-border-strong);
    stroke-width: 2;
  }

  :global(.svelte-flow__controls),
  :global(.svelte-flow__minimap) {
    border: 2px solid var(--wm-border-strong);
    border-radius: 0;
    background: var(--wm-surface);
  }
</style>
