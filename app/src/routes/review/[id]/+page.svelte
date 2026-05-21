<script lang="ts">
  import { page } from '$app/stores';
  import { tick } from 'svelte';
  import hljs from 'highlight.js/lib/core';
  import bash from 'highlight.js/lib/languages/bash';
  import css from 'highlight.js/lib/languages/css';
  import javascript from 'highlight.js/lib/languages/javascript';
  import json from 'highlight.js/lib/languages/json';
  import markdown from 'highlight.js/lib/languages/markdown';
  import python from 'highlight.js/lib/languages/python';
  import rust from 'highlight.js/lib/languages/rust';
  import toml from 'highlight.js/lib/languages/ini';
  import typescript from 'highlight.js/lib/languages/typescript';
  import xml from 'highlight.js/lib/languages/xml';
  import yaml from 'highlight.js/lib/languages/yaml';
  import {
    api,
    apiBase,
    type FeedbackBatch,
    type ReviewFile,
    type ReviewSession,
    type ReviewDiffLine,
    type ReviewThreadRecord
  } from '$lib/api';

  hljs.registerLanguage('bash', bash);
  hljs.registerLanguage('css', css);
  hljs.registerLanguage('javascript', javascript);
  hljs.registerLanguage('json', json);
  hljs.registerLanguage('markdown', markdown);
  hljs.registerLanguage('python', python);
  hljs.registerLanguage('rust', rust);
  hljs.registerLanguage('toml', toml);
  hljs.registerLanguage('typescript', typescript);
  hljs.registerLanguage('xml', xml);
  hljs.registerLanguage('yaml', yaml);

  let session: ReviewSession | null = null;
  let files: ReviewFile[] = [];
  let threads: ReviewThreadRecord[] = [];
  let selected: ReviewDiffLine | null = null;
  let rangeStart: ReviewDiffLine | null = null;
  let rangeEnd: ReviewDiffLine | null = null;
  let selectedLines: ReviewDiffLine[] = [];
  let selectedLineIds = new Set<string>();
  let selectionLabel = 'Select a diff line.';
  let selectingRange = false;
  let composerAnchorLineId: string | null = null;
  let composerOpen = false;
  let threadType = 'comment';
  let threadBody = '';
  let threadVisibility = 'agent';
  let replyBodies: Record<string, string> = {};
  let activeThreadId: string | null = null;
  let message = '';
  let filter = '';
  let leftWidth = 300;
  let filesHidden = false;
  const collapsed = new Set<string>();
  const skipped = new Set<string>();
  const revealedLarge = new Set<string>();
  const largeChangeThreshold = 500;
  let lastBatch: FeedbackBatch | null = null;
  let dispatching = false;
  let batchPanelOpen = false;

  $: sessionId = $page.params.id;
  $: gridColumns = `${filesHidden ? 0 : leftWidth}px 8px minmax(0, 1fr)`;
  $: visibleFiles = files.filter((item) =>
    item.file.path.toLowerCase().includes(filter.toLowerCase())
  );
  $: selectedLines = getSelectedRangeLines(files, selected, rangeStart, rangeEnd);
  $: selectedLineIds = new Set(selectedLines.map((line) => line.id));
  $: selectionLabel = formatRangeLabel(selectedLines);
  $: threadsByLine = groupThreadsByLine(threads);
  $: pendingThreadCount = threads.filter(isThreadPending).length;

  function isThreadPending(thread: ReviewThreadRecord) {
    if (thread.status !== 'open') return false;
    const visible = thread.messages.filter((message) => message.visibility !== 'private');
    if (visible.length === 0) return false;
    const watermark = thread.last_delivered_message_id;
    if (!watermark) {
      return visible.some((message) => message.author_kind === 'human');
    }
    const index = visible.findIndex((message) => message.id === watermark);
    if (index < 0) return visible.some((message) => message.author_kind === 'human');
    return visible.slice(index + 1).some((message) => message.author_kind === 'human');
  }

  async function load() {
    const sessionData = await api<{ review_session: ReviewSession }>(
      `/api/review-sessions/${sessionId}`
    );
    const diffData = await api<{ files: ReviewFile[] }>(`/api/review-sessions/${sessionId}/diff`);
    const threadData = await api<{ threads: ReviewThreadRecord[] }>(
      `/api/review-sessions/${sessionId}/threads`
    );
    session = sessionData.review_session;
    files = diffData.files;
    threads = threadData.threads;
  }

  function toggle(set: Set<string>, id: string) {
    if (set.has(id)) set.delete(id);
    else set.add(id);
    files = files;
  }

  function scrollDiffTo(fileId: string) {
    requestAnimationFrame(() => {
      const target = document.getElementById(fileId);
      const container = document.querySelector<HTMLElement>('section.diff');
      if (!target || !container) return;
      const stickyOffset =
        container.querySelector<HTMLElement>('.session-bar')?.offsetHeight ?? 0;
      const top = target.offsetTop - container.offsetTop - stickyOffset - 4;
      container.scrollTo({ top, behavior: 'smooth' });
    });
  }

  function changeCount(item: ReviewFile) {
    return item.file.additions + item.file.deletions;
  }

  function isLargeFile(item: ReviewFile) {
    return changeCount(item) >= largeChangeThreshold;
  }

  function isLargeHidden(item: ReviewFile) {
    return isLargeFile(item) && !revealedLarge.has(item.file.id);
  }

  function showLargeFile(id: string) {
    revealedLarge.add(id);
    files = files;
  }

  function languageForPath(path: string) {
    const lower = path.toLowerCase();
    if (lower.endsWith('.rs')) return 'rust';
    if (lower.endsWith('.ts') || lower.endsWith('.svelte')) return 'typescript';
    if (lower.endsWith('.js') || lower.endsWith('.mjs') || lower.endsWith('.cjs')) {
      return 'javascript';
    }
    if (lower.endsWith('.json') || lower.endsWith('.lock')) return 'json';
    if (lower.endsWith('.toml')) return 'toml';
    if (lower.endsWith('.yml') || lower.endsWith('.yaml')) return 'yaml';
    if (lower.endsWith('.md') || lower.endsWith('.mdx')) return 'markdown';
    if (lower.endsWith('.py')) return 'python';
    if (lower.endsWith('.css')) return 'css';
    if (lower.endsWith('.sh') || lower.endsWith('.zsh') || lower.endsWith('.bash')) return 'bash';
    if (
      lower.endsWith('.html') ||
      lower.endsWith('.xml') ||
      lower.endsWith('.svg') ||
      lower.endsWith('.vue')
    ) {
      return 'xml';
    }
    return null;
  }

  function highlightedLine(line: ReviewDiffLine) {
    const marker = line.content.slice(0, 1);
    const source =
      marker === '+' || marker === '-' || marker === ' ' ? line.content.slice(1) : line.content;
    const language = languageForPath(line.file_path);
    const highlighted =
      language && hljs.getLanguage(language)
        ? hljs.highlight(source, { language, ignoreIllegals: true }).value
        : escapeHtml(source);
    return `<span class="diff-marker">${escapeHtml(marker)}</span><span class="syntax">${highlighted}</span>`;
  }

  function escapeHtml(value: string) {
    return value
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;')
      .replaceAll('"', '&quot;')
      .replaceAll("'", '&#39;');
  }

  function startResize(event: MouseEvent) {
    event.preventDefault();
    const startX = event.clientX;
    const startLeft = leftWidth;

    function onMove(moveEvent: MouseEvent) {
      leftWidth = clamp(startLeft + moveEvent.clientX - startX, 180, 520);
    }

    function onUp() {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
      document.body.classList.remove('resizing-pane');
    }

    document.body.classList.add('resizing-pane');
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  function clamp(value: number, min: number, max: number) {
    return Math.min(max, Math.max(min, value));
  }

  function allLinesForFile(filePath: string, fileDiffs = files) {
    return fileDiffs
      .flatMap((item) => item.hunks.flatMap((hunk) => hunk.lines))
      .filter((line) => line.file_path === filePath);
  }

  function getSelectedRangeLines(
    fileDiffs: ReviewFile[],
    selectedLine: ReviewDiffLine | null,
    startLine: ReviewDiffLine | null,
    endLine: ReviewDiffLine | null
  ) {
    if (!startLine || !endLine || startLine.file_path !== endLine.file_path) {
      return selectedLine ? [selectedLine] : [];
    }
    const lines = allLinesForFile(startLine.file_path, fileDiffs);
    const start = lines.findIndex((item) => item.id === startLine.id);
    const end = lines.findIndex((item) => item.id === endLine.id);
    if (start < 0 || end < 0) return selectedLine ? [selectedLine] : [];
    const low = Math.min(start, end);
    const high = Math.max(start, end);
    return lines.slice(low, high + 1);
  }

  function formatRangeLabel(lines: ReviewDiffLine[]) {
    if (lines.length === 0) return 'Select a diff line.';
    const first = lines[0];
    const last = lines[lines.length - 1];
    const firstLine = first.new_line ?? first.old_line ?? '?';
    const lastLine = last.new_line ?? last.old_line ?? firstLine;
    return lines.length === 1
      ? `${first.file_path}:${firstLine}`
      : `${first.file_path}:${firstLine}-${lastLine}`;
  }

  function startLineSelection(line: ReviewDiffLine, event: MouseEvent) {
    event.preventDefault();
    selected = line;
    rangeStart = line;
    rangeEnd = line;
    selectingRange = true;

    function onUp() {
      selectingRange = false;
      composerAnchorLineId = rangeEnd?.id ?? line.id;
      composerOpen = true;
      window.removeEventListener('mouseup', onUp);
      document.body.classList.remove('selecting-range');
      void focusComposer();
    }

    document.body.classList.add('selecting-range');
    window.addEventListener('mouseup', onUp);
  }

  function openLineComposer(line: ReviewDiffLine, event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    selected = line;
    rangeStart = line;
    rangeEnd = line;
    selectingRange = false;
    composerAnchorLineId = line.id;
    composerOpen = true;
    void focusComposer();
  }

  function updateLineSelection(line: ReviewDiffLine) {
    if (!selectingRange || !rangeStart || line.file_path !== rangeStart.file_path) return;
    rangeEnd = line;
    selected = line;
    composerAnchorLineId = line.id;
  }

  function isLineRangeSelected(line: ReviewDiffLine) {
    return selectedLineIds.has(line.id);
  }

  function rangePayload() {
    const lines = selectedLines;
    if (lines.length === 0) return null;
    const first = lines[0];
    const last = lines[lines.length - 1];
    return {
      file_path: first.file_path,
      anchor_diff_line_id: first.id,
      old_line: first.old_line,
      new_line: first.new_line,
      range_start_old_line: first.old_line,
      range_start_new_line: first.new_line,
      range_end_old_line: last.old_line,
      range_end_new_line: last.new_line,
      selected_text: lines.map((line) => line.content).join('\n')
    };
  }

  async function focusComposer() {
    await tick();
    document.getElementById('inline-thread-body')?.focus();
  }

  function groupThreadsByLine(items: ReviewThreadRecord[]) {
    const map = new Map<string, ReviewThreadRecord[]>();
    for (const thread of items) {
      const key = thread.anchor_diff_line_id;
      if (!key) continue;
      const list = map.get(key);
      if (list) list.push(thread);
      else map.set(key, [thread]);
    }
    return map;
  }

  function messageTypeLabel(type: string) {
    return type
      .split('_')
      .map((part) => part.slice(0, 1).toUpperCase() + part.slice(1))
      .join(' ');
  }

  async function createThread() {
    const range = rangePayload();
    if (!range || !threadBody.trim()) return;
    const data = await api<{ thread: ReviewThreadRecord }>(
      `/api/review-sessions/${sessionId}/threads`,
      {
        method: 'POST',
        body: {
          ...range,
          body: threadBody,
          message_type: threadType,
          status: 'open',
          visibility: threadVisibility
        }
      }
    );
    threads = [...threads, data.thread];
    message = 'Thread saved.';
    threadBody = '';
    selected = null;
    rangeStart = null;
    rangeEnd = null;
    composerAnchorLineId = null;
    composerOpen = false;
  }

  function closeComposer() {
    selected = null;
    rangeStart = null;
    rangeEnd = null;
    composerAnchorLineId = null;
    composerOpen = false;
    threadBody = '';
  }

  async function deleteThread(thread: ReviewThreadRecord) {
    if (!confirm('Delete this thread and all its messages?')) return;
    await api(`/api/review-threads/${thread.id}`, { method: 'DELETE' });
    threads = threads.filter((item) => item.id !== thread.id);
    if (activeThreadId === thread.id) activeThreadId = null;
  }

  async function resolveThread(thread: ReviewThreadRecord) {
    await api(`/api/review-threads/${thread.id}/resolve`, { method: 'POST' });
    threads = threads.map((item) =>
      item.id === thread.id ? { ...item, status: 'resolved' } : item
    );
  }

  async function reopenThread(thread: ReviewThreadRecord) {
    await api(`/api/review-threads/${thread.id}/reopen`, { method: 'POST' });
    threads = threads.map((item) => (item.id === thread.id ? { ...item, status: 'open' } : item));
  }

  async function dispatchBatch() {
    if (dispatching) return;
    dispatching = true;
    message = 'Building feedback batch...';
    try {
      const data = await api<{ batch: FeedbackBatch }>(
        `/api/review-sessions/${sessionId}/feedback-batches`,
        { method: 'POST', body: {} }
      );
      lastBatch = data.batch;
      batchPanelOpen = true;
      message =
        data.batch.thread_count === 0
          ? 'No threads to dispatch.'
          : `Batch queued: ${data.batch.thread_count} thread(s). Run wyrd_diff.pending_feedback in your agent.`;
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
    } finally {
      dispatching = false;
    }
  }

  async function copyBatchPayload() {
    if (!lastBatch) return;
    await navigator.clipboard.writeText(lastBatch.payload);
    message = 'Markdown payload copied.';
  }

  let refreshing = false;

  async function refreshDiff() {
    if (refreshing) return;
    refreshing = true;
    message = 'Refreshing diff...';
    try {
      await api(`/api/review-sessions/${sessionId}/refresh`, { method: 'POST', body: {} });
      await load();
      message = 'Diff refreshed.';
    } catch (error) {
      message = `Refresh failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      refreshing = false;
    }
  }

  async function addReply(thread: ReviewThreadRecord) {
    const body = replyBodies[thread.id]?.trim();
    if (!body) return;
    const data = await api<{ message: ReviewThreadRecord['messages'][number] }>(
      `/api/review-threads/${thread.id}/messages`,
      {
        method: 'POST',
        body: {
          body,
          message_type: 'comment',
          author_kind: 'human',
          status: 'open',
          visibility: thread.visibility
        }
      }
    );
    threads = threads.map((item) =>
      item.id === thread.id ? { ...item, messages: [...item.messages, data.message] } : item
    );
    replyBodies = { ...replyBodies, [thread.id]: '' };
    activeThreadId = thread.id;
  }

  load().catch((error) => {
    message = error.message;
  });
</script>

<main class="review" class:files-hidden={filesHidden} style:grid-template-columns={gridColumns}>
  <nav class="files" aria-hidden={filesHidden}>
    <a class="home" href="/">Wyrd Diff</a>
    <button class="pane-toggle" on:click={() => (filesHidden = true)}>Hide files</button>
    <input bind:value={filter} placeholder="Filter files" />
    {#each visibleFiles as item (item.file.id)}
      <button
        class:skipped={skipped.has(item.file.id)}
        class:large={isLargeFile(item)}
        class:hidden-large={isLargeHidden(item)}
        on:click={() => {
          skipped.delete(item.file.id);
          collapsed.delete(item.file.id);
          files = files;
          scrollDiffTo(item.file.id);
        }}
      >
        <span>{item.file.path}</span>
        <small>
          +{item.file.additions} -{item.file.deletions}
          {#if isLargeHidden(item)}
            <em>hidden</em>
          {/if}
        </small>
      </button>
    {/each}
  </nav>

  <button class="resizer left-resizer" aria-label="Resize file sidebar" on:mousedown={startResize}
  ></button>

  <section class="diff">
    <div class="session-bar">
      <strong>Session ID</strong>
      <code>{sessionId}</code>
      <div class="session-bar-spacer"></div>
      <button
        class="refresh-button"
        disabled={refreshing}
        title={session?.head_ref === 'WORKTREE'
          ? 'Re-run git diff against the current working tree'
          : 'Re-resolve refs and rebuild the diff snapshot'}
        on:click={refreshDiff}
      >
        {refreshing ? 'Refreshing…' : 'Refresh diff'}
      </button>
      <button
        class="dispatch-button"
        class:armed={pendingThreadCount > 0}
        disabled={dispatching || pendingThreadCount === 0}
        title={pendingThreadCount === 0
          ? 'No open threads with new reviewer input'
          : 'Queue a feedback batch for wyrd_diff.pending_feedback'}
        on:click={dispatchBatch}
      >
        {dispatching ? 'Dispatching...' : `Dispatch batch (${pendingThreadCount})`}
      </button>
      {#if lastBatch}
        <button
          class="dispatch-button"
          on:click={() => (batchPanelOpen = !batchPanelOpen)}
          title="Show queued batch markdown"
        >
          {batchPanelOpen ? 'Hide payload' : 'Show payload'}
        </button>
      {/if}
    </div>
    {#if batchPanelOpen && lastBatch}
      <section class="batch-panel">
        <header>
          <div>
            <strong>Feedback batch</strong>
            <code>{lastBatch.id.slice(0, 8)}</code>
            <span class="badge {lastBatch.status}">{lastBatch.status}</span>
            <span>{lastBatch.thread_count} thread(s)</span>
            {#if lastBatch.delivered_at}
              <span>delivered {lastBatch.delivered_at}</span>
            {:else}
              <span>awaiting agent pull</span>
            {/if}
          </div>
          <div>
            <button on:click={copyBatchPayload}>Copy markdown</button>
            <button on:click={() => (batchPanelOpen = false)}>Close</button>
          </div>
        </header>
        <pre>{lastBatch.payload}</pre>
        <p class="batch-hint">
          In your running agent, call <code>wyrd_diff.pending_feedback</code> with
          <code>session_id</code> = <code>{sessionId}</code>. The first call returns this markdown;
          subsequent calls only return new replies past the watermark.
        </p>
      </section>
    {/if}
    <div class="layout-controls">
      <button on:click={() => (filesHidden = !filesHidden)}>
        {filesHidden ? 'Show files' : 'Hide files'}
      </button>
      {#if message}<p class="message">{message}</p>{/if}
    </div>
    {#if session}
      <header>
        <h1>
          {session.title}
          {#if session.head_ref === 'WORKTREE'}
            <span class="badge uncommitted" title="Includes staged + unstaged tracked changes"
              >uncommitted</span
            >
          {/if}
        </h1>
        <p>
          {session.base_ref} ..
          {#if session.head_ref === 'WORKTREE'}
            <span class="worktree-ref">working tree</span>
            <small>({session.head_sha.slice(0, 7)} + uncommitted)</small>
          {:else}
            {session.head_ref}
          {/if}
        </p>
        <code>{apiBase}/api/review-sessions/{session.id}/agent-context</code>
      </header>
    {/if}

    {#each files as item (item.file.id)}
      {#if !skipped.has(item.file.id)}
        <article id={item.file.id} class:collapsed={collapsed.has(item.file.id)}>
          <h2>
            <span>{item.file.path}</span>
            <span>
              {#if isLargeHidden(item)}
                <button on:click={() => showLargeFile(item.file.id)}>Show diff</button>
              {:else}
                <button on:click={() => toggle(collapsed, item.file.id)}>
                  {collapsed.has(item.file.id) ? 'Expand' : 'Collapse'}
                </button>
              {/if}
              <button on:click={() => toggle(skipped, item.file.id)}>Skip</button>
            </span>
          </h2>
          {#if isLargeHidden(item)}
            <div class="large-diff">
              <strong>Large diff hidden</strong>
              <p>
                {changeCount(item)} changed lines. Generated files, lockfiles, and broad mechanical changes
                stay hidden until you choose to inspect them.
              </p>
              <button on:click={() => showLargeFile(item.file.id)}>Show diff</button>
            </div>
          {:else if !collapsed.has(item.file.id)}
            {#each item.hunks as hunk (hunk.id)}
              <table>
                <tbody>
                  {#each hunk.lines as line (line.id)}
                    <tr
                      class={line.line_kind}
                      class:selected={selected?.id === line.id}
                      class:range-selected={isLineRangeSelected(line)}
                      on:mousedown={(event) => startLineSelection(line, event)}
                      on:mouseenter={() => updateLineSelection(line)}
                    >
                      <td class="num line-action">
                        <button
                          class="add-thread"
                          aria-label={`Add thread on ${line.file_path}:${line.new_line ?? line.old_line ?? ''}`}
                          title="Add thread"
                          on:mousedown={(event) => {
                            event.preventDefault();
                            event.stopPropagation();
                          }}
                          on:click={(event) => openLineComposer(line, event)}
                        >
                          +
                        </button>
                        <span>{line.old_line ?? ''}</span>
                      </td>
                      <td class="num">{line.new_line ?? ''}</td>
                      <!-- eslint-disable-next-line svelte/no-at-html-tags -- highlightedLine escapes raw text before injecting syntax spans -->
                      <td class="code">{@html highlightedLine(line)}</td>
                    </tr>
                    {#if composerOpen && selectedLines.length > 0 && composerAnchorLineId === line.id}
                      <tr class="composer-row">
                        <td colspan="3">
                          <section class="inline-composer">
                            <div class="inline-composer-header">
                              <div>
                                <p>Add thread</p>
                                <h3>{selectionLabel}</h3>
                              </div>
                              <button aria-label="Close composer" on:click={closeComposer}>x</button
                              >
                            </div>
                            <div class="thread-meta">
                              <span
                                >{selectedLines.length} line{selectedLines.length === 1
                                  ? ''
                                  : 's'}</span
                              >
                              {#if composerAnchorLineId}
                                <span>anchored</span>
                              {/if}
                            </div>
                            <div class="composer-controls">
                              <select bind:value={threadType}>
                                <option value="comment">Comment</option>
                                <option value="thinking_note">Thinking note</option>
                                <option value="decision">Decision</option>
                                <option value="agent_instruction">Agent instruction</option>
                              </select>
                              <select bind:value={threadVisibility}>
                                <option value="agent">Agent-visible</option>
                                <option value="private">Private</option>
                              </select>
                            </div>
                            <textarea
                              id="inline-thread-body"
                              bind:value={threadBody}
                              placeholder="Write a message"
                              on:keydown={(event) => {
                                if (event.key === 'Escape') closeComposer();
                              }}
                            ></textarea>
                            <div class="composer-actions">
                              <button on:click={createThread}>Save thread</button>
                              <button on:click={closeComposer}>Cancel</button>
                            </div>
                          </section>
                        </td>
                      </tr>
                    {/if}
                    {#each threadsByLine.get(line.id) ?? [] as thread (thread.id)}
                      <tr class="thread-row">
                        <td colspan="3">
                          <section
                            class="thread"
                            class:active={activeThreadId === thread.id}
                            class:resolved={thread.status === 'resolved'}
                            class:pending={isThreadPending(thread)}
                          >
                            <div class="thread-meta">
                              <strong>{formatRangeLabel([line])}</strong>
                              <span class="badge {thread.status}">{thread.status}</span>
                              <span>{thread.visibility}</span>
                              {#if isThreadPending(thread)}
                                <span
                                  class="badge pending"
                                  title="Has new reviewer input past last delivery">queued</span
                                >
                              {/if}
                              {#if thread.status === 'open'}
                                <button
                                  class="thread-action"
                                  title="Mark resolved"
                                  on:click={() => resolveThread(thread)}>Resolve</button
                                >
                              {:else}
                                <button
                                  class="thread-action"
                                  title="Reopen thread"
                                  on:click={() => reopenThread(thread)}>Reopen</button
                                >
                              {/if}
                              <button
                                class="thread-delete"
                                aria-label="Delete thread"
                                title="Delete thread"
                                on:click={() => deleteThread(thread)}>Delete</button
                              >
                            </div>
                            {#each thread.messages as threadMessage (threadMessage.id)}
                              <article
                                class="thread-message"
                                class:agent={threadMessage.author_kind === 'agent'}
                              >
                                <div>
                                  <strong
                                    >{threadMessage.author_name ??
                                      threadMessage.author_kind}</strong
                                  >
                                  <span>{messageTypeLabel(threadMessage.message_type)}</span>
                                  {#if threadMessage.fix_import_id}
                                    <code>fix {threadMessage.fix_import_id.slice(0, 8)}</code>
                                  {/if}
                                </div>
                                <p>{threadMessage.body}</p>
                              </article>
                            {/each}
                            <div class="reply">
                              <textarea
                                bind:value={replyBodies[thread.id]}
                                placeholder="Follow up for the agent"
                                on:focus={() => (activeThreadId = thread.id)}
                              ></textarea>
                              <button on:click={() => addReply(thread)}>Reply</button>
                            </div>
                          </section>
                        </td>
                      </tr>
                    {/each}
                  {/each}
                </tbody>
              </table>
            {/each}
          {/if}
        </article>
      {/if}
    {/each}
  </section>
</main>

<style>
  :global(body) {
    overflow: hidden;
  }

  :global(body.resizing-pane) {
    cursor: col-resize;
    user-select: none;
  }

  :global(body.selecting-range) {
    user-select: none;
  }

  :global(html, body) {
    height: 100%;
    overflow: hidden;
  }

  .review {
    display: grid;
    height: 100vh;
    max-height: 100vh;
    overflow: hidden;
    background: var(--wm-bg);
    font-size: 13px;
  }

  .files {
    position: sticky;
    grid-column: 1;
    top: 0;
    height: 100vh;
    overflow: auto;
    background: var(--wm-surface);
    padding: 14px;
    border-right: 1px solid var(--wm-border-strong);
    box-shadow: 2px 0 16px rgba(124, 255, 158, 0.06);
  }

  .review.files-hidden .files {
    display: none;
  }

  .resizer {
    position: sticky;
    top: 0;
    z-index: 3;
    width: 8px;
    min-width: 8px;
    height: 100vh;
    min-height: 0;
    padding: 0;
    border: 0;
    border-left: 1px solid var(--wm-border);
    border-right: 1px solid var(--wm-border);
    background: var(--wm-black);
    box-shadow: none;
    cursor: col-resize;
  }

  .resizer:hover {
    background: rgba(124, 255, 158, 0.15);
    box-shadow: 0 0 14px rgba(124, 255, 158, 0.4);
    transform: none;
    border-color: var(--wm-green);
  }

  .left-resizer {
    grid-column: 2;
  }

  .pane-toggle {
    width: 100%;
    margin: 0 0 10px;
  }

  .home {
    display: block;
    margin-bottom: 14px;
    padding: 9px 10px;
    border: 1px solid var(--wm-green);
    color: var(--wm-green);
    font-weight: 900;
    font-size: 18px;
    line-height: 1;
    text-decoration: none;
    box-shadow: var(--wm-glow-green-sm);
    text-shadow: 0 0 10px rgba(124, 255, 158, 0.6);
    letter-spacing: 0.04em;
    transition: box-shadow 0.12s;
  }

  .home:hover {
    box-shadow: var(--wm-glow-green);
  }

  input,
  select,
  textarea {
    width: 100%;
    margin: 8px 0;
    padding: 8px;
    font-size: 13px;
    line-height: 1.35;
  }

  textarea {
    min-height: 90px;
    resize: vertical;
  }

  select {
    min-height: 34px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-ink);
    font-weight: 800;
  }

  button {
    min-height: 30px;
    padding: 5px 8px;
    font-size: 12px;
    line-height: 1;
  }

  .files button {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    width: 100%;
    margin: 6px 0;
    border-color: var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-ink);
    font-size: 12px;
    font-weight: 700;
    line-height: 1.2;
    box-shadow: none;
    text-align: left;
    transition:
      border-color 0.1s,
      box-shadow 0.1s;
  }

  .files button:hover {
    border-color: var(--wm-green);
    box-shadow: 0 0 8px rgba(124, 255, 158, 0.25);
  }

  .files button.skipped {
    opacity: 0.55;
    text-decoration: line-through;
  }

  .files button.large {
    border-style: dashed;
  }

  .files button.hidden-large {
    border-color: var(--wm-amber);
  }

  .files span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .files small {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    color: var(--wm-muted);
    white-space: nowrap;
  }

  .files em {
    color: var(--wm-amber);
    font-style: normal;
    text-transform: uppercase;
  }

  .diff {
    position: relative;
    isolation: isolate;
    grid-column: 3;
    min-width: 0;
    height: 100vh;
    padding: 0 8px 16px;
    overflow: auto;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
    background: var(--wm-bg);
  }

  .session-bar {
    position: sticky;
    top: 0;
    z-index: 40;
    display: flex;
    gap: 10px;
    align-items: center;
    min-height: 44px;
    margin: 0 -16px 12px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--wm-border-strong);
    background: var(--wm-surface);
    font: 12px/1.25 var(--wm-mono);
  }

  .session-bar strong {
    color: var(--wm-green);
    text-transform: uppercase;
    text-shadow: 0 0 8px rgba(124, 255, 158, 0.6);
    letter-spacing: 0.06em;
  }

  .session-bar-spacer {
    flex: 1;
  }

  .dispatch-button {
    min-height: 26px;
    padding: 4px 10px;
    border: 1px solid var(--wm-border-strong);
    background: var(--wm-bg);
    color: var(--wm-ink);
    box-shadow: none;
    font: 700 11px/1.2 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    white-space: nowrap;
  }

  .dispatch-button.armed {
    color: var(--wm-amber);
    border-color: var(--wm-amber);
    box-shadow: var(--wm-glow-amber);
  }

  .dispatch-button:disabled {
    color: var(--wm-subtle);
    border-color: var(--wm-border);
    box-shadow: none;
    cursor: not-allowed;
  }

  .dispatch-button:not(:disabled):hover {
    border-color: var(--wm-green);
    box-shadow: var(--wm-glow-green-sm);
  }

  .refresh-button {
    min-height: 26px;
    padding: 4px 10px;
    margin-right: 6px;
    border: 1px solid var(--wm-border-strong);
    background: var(--wm-bg);
    color: var(--wm-ink);
    font: 700 11px/1.2 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    white-space: nowrap;
  }

  .refresh-button:not(:disabled):hover {
    border-color: var(--wm-green);
    color: var(--wm-green);
    box-shadow: var(--wm-glow-green-sm);
  }

  .refresh-button:disabled {
    color: var(--wm-subtle);
    border-color: var(--wm-border);
    cursor: not-allowed;
  }

  .batch-panel {
    margin: 0 0 12px;
    padding: 12px;
    border: 1px solid var(--wm-amber);
    background: var(--wm-surface);
    box-shadow: var(--wm-glow-amber);
    font: 12px/1.45 var(--wm-mono);
  }

  .batch-panel header {
    display: flex;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    margin: 0 0 8px;
    padding: 0 0 8px;
    border: 0;
    border-bottom: 1px solid var(--wm-border);
    background: transparent;
    box-shadow: none;
  }

  .batch-panel header > div {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }

  .batch-panel header strong {
    color: var(--wm-green);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .batch-panel pre {
    max-height: 320px;
    margin: 0 0 8px;
    padding: 10px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-ink);
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .batch-hint {
    margin: 0;
    color: var(--wm-muted);
    font-size: 11px;
  }

  .batch-hint code {
    padding: 1px 4px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-green);
  }

  .layout-controls {
    display: flex;
    gap: 8px;
    justify-content: flex-start;
    align-items: center;
    margin: 0 0 12px;
    padding: 5px;
    border: 1px solid var(--wm-border);
    background: var(--wm-surface);
  }

  .layout-controls button {
    min-height: 24px;
    padding: 4px 7px;
    border: 1px solid var(--wm-border-strong);
    background: var(--wm-ink);
    color: var(--wm-black);
    box-shadow: var(--wm-glow-green-sm);
    font-size: 11px;
    font-weight: 800;
    line-height: 1;
    text-decoration: none;
    white-space: nowrap;
  }

  .layout-controls button:hover {
    box-shadow: var(--wm-glow-green);
  }

  header,
  article {
    margin-bottom: 16px;
    border: 1px solid var(--wm-border-strong);
    background: var(--wm-surface);
    box-shadow: 0 0 16px rgba(124, 255, 158, 0.06);
  }

  article {
    content-visibility: auto;
    contain-intrinsic-size: 600px 1200px;
    contain: layout paint style;
  }

  article.collapsed {
    contain-intrinsic-size: 600px 60px;
  }

  header {
    padding: 14px;
    border-color: var(--wm-green);
    box-shadow: 0 0 18px rgba(124, 255, 158, 0.18);
  }

  header h1 {
    color: var(--wm-green);
    text-shadow: 0 0 14px rgba(124, 255, 158, 0.7);
    letter-spacing: 0.02em;
  }

  h1,
  h2,
  p {
    margin: 0 0 8px;
  }

  h2 {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--wm-border);
    font-size: 13px;
    line-height: 1.3;
  }

  h2 > span:first-child {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .large-diff {
    display: grid;
    gap: 10px;
    padding: 16px;
    border-top: 1px dashed var(--wm-border);
    background: var(--wm-bg);
  }

  .large-diff strong {
    color: var(--wm-amber);
    font-size: 13px;
    text-transform: uppercase;
  }

  .large-diff p {
    max-width: 680px;
    color: var(--wm-muted);
    font: 12px/1.45 var(--wm-mono);
  }

  .large-diff button {
    justify-self: start;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
  }

  tr {
    cursor: pointer;
  }

  tr.add {
    background: var(--wm-green-bg);
    box-shadow: inset 3px 0 0 var(--wm-green);
  }

  tr.del {
    background: var(--wm-red-bg);
    box-shadow: inset 3px 0 0 var(--wm-red);
  }

  tr.selected {
    background: var(--wm-amber-bg);
    outline: 1px solid var(--wm-amber);
    outline-offset: -1px;
    box-shadow:
      inset 0 0 0 1px var(--wm-amber),
      0 0 16px rgba(255, 209, 102, 0.35);
  }

  tr.range-selected {
    background: rgba(255, 209, 102, 0.24);
  }

  tr.range-selected td {
    border-top: 1px solid rgba(255, 209, 102, 0.38);
    border-bottom: 1px solid rgba(255, 209, 102, 0.38);
  }

  tr.thread-row,
  tr.composer-row {
    cursor: default;
  }

  tr.thread-row td,
  tr.composer-row td {
    background: var(--wm-bg);
  }

  td {
    vertical-align: top;
    font:
      12px/1.55 ui-monospace,
      SFMono-Regular,
      Menlo,
      Consolas,
      monospace;
  }

  tr:hover td {
    background: rgba(124, 255, 158, 0.08);
  }

  .num {
    width: 38px;
    padding: 0 6px;
    color: var(--wm-subtle);
    text-align: right;
    user-select: none;
  }

  .line-action {
    position: relative;
  }

  .line-action span {
    display: block;
  }

  .add-thread {
    position: absolute;
    top: 50%;
    left: 2px;
    z-index: 2;
    width: 18px;
    min-width: 18px;
    min-height: 18px;
    padding: 0;
    border: 1px solid var(--wm-border-strong);
    background: var(--wm-ink);
    color: var(--wm-black);
    box-shadow: var(--wm-glow-green-sm);
    font-size: 13px;
    line-height: 14px;
    opacity: 0;
    transform: translateY(-50%);
  }

  tr:hover .add-thread,
  tr.selected .add-thread,
  tr.range-selected .add-thread {
    opacity: 1;
  }

  .add-thread:hover {
    background: var(--wm-amber);
    box-shadow: var(--wm-glow-amber);
    transform: translateY(-50%);
  }

  .code {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    padding: 0 8px;
  }

  .code :global(.diff-marker) {
    display: inline-block;
    width: 1ch;
    color: var(--wm-muted);
  }

  tr.add .code :global(.diff-marker),
  tr.add .code :global(.hljs-addition) {
    color: var(--wm-green);
  }

  tr.del .code :global(.diff-marker),
  tr.del .code :global(.hljs-deletion) {
    color: var(--wm-red);
  }

  .code :global(.hljs-keyword),
  .code :global(.hljs-selector-tag),
  .code :global(.hljs-built_in),
  .code :global(.hljs-type),
  .code :global(.hljs-literal) {
    color: var(--wm-orange);
  }

  .code :global(.hljs-title),
  .code :global(.hljs-title.function_),
  .code :global(.hljs-section),
  .code :global(.hljs-attr) {
    color: var(--wm-blue);
  }

  .code :global(.hljs-string),
  .code :global(.hljs-regexp),
  .code :global(.hljs-symbol) {
    color: var(--wm-green);
  }

  .code :global(.hljs-number),
  .code :global(.hljs-boolean),
  .code :global(.hljs-variable),
  .code :global(.hljs-template-variable) {
    color: var(--wm-amber);
  }

  .code :global(.hljs-comment),
  .code :global(.hljs-quote) {
    color: var(--wm-subtle);
    font-style: italic;
  }

  .code :global(.hljs-meta),
  .code :global(.hljs-tag),
  .code :global(.hljs-name),
  .code :global(.hljs-attribute) {
    color: var(--wm-purple);
  }

  .thread {
    margin: 8px 10px 12px;
    padding: 10px;
    border: 1px solid var(--wm-border);
    background: var(--wm-surface);
    box-shadow: 0 0 10px rgba(124, 255, 158, 0.06);
  }

  .thread.active {
    border-color: var(--wm-amber);
    box-shadow: var(--wm-glow-amber);
  }

  .thread-meta,
  .composer-controls,
  .composer-actions {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }

  .thread-meta {
    margin-bottom: 8px;
    color: var(--wm-muted);
    font: 11px/1.3 var(--wm-mono);
  }

  .thread-meta strong {
    color: var(--wm-ink);
  }

  .thread-action,
  .thread-delete {
    min-height: 22px;
    padding: 2px 8px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    box-shadow: none;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .thread-action {
    color: var(--wm-green);
    margin-left: auto;
  }

  .thread-action:hover {
    border-color: var(--wm-green);
    box-shadow: var(--wm-glow-green-sm);
  }

  .thread-delete {
    color: var(--wm-red);
  }

  .thread-meta .thread-action ~ .thread-delete {
    margin-left: 0;
  }

  .thread-meta .thread-delete:only-of-type {
    margin-left: auto;
  }

  .thread-delete:hover {
    border-color: var(--wm-red);
    box-shadow: 0 0 10px rgba(255, 90, 60, 0.4);
  }

  .thread.resolved {
    opacity: 0.55;
  }

  .thread.resolved .thread-meta strong {
    text-decoration: line-through;
  }

  .thread.pending {
    border-color: var(--wm-amber);
    box-shadow: 0 0 12px rgba(255, 209, 102, 0.18);
  }

  .badge {
    padding: 2px 6px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-muted);
    font: 700 10px/1.2 var(--wm-mono);
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .badge.open {
    color: var(--wm-green);
    border-color: var(--wm-border-strong);
  }

  .badge.resolved {
    color: var(--wm-subtle);
  }

  .badge.pending {
    color: var(--wm-amber);
    border-color: var(--wm-amber);
    text-shadow: 0 0 6px rgba(255, 209, 102, 0.5);
  }

  .badge.delivered {
    color: var(--wm-blue);
    border-color: var(--wm-border-strong);
  }

  .badge.superseded {
    color: var(--wm-subtle);
  }

  .badge.uncommitted {
    margin-left: 10px;
    padding: 3px 8px;
    color: var(--wm-amber);
    border-color: var(--wm-amber);
    background: rgba(255, 209, 102, 0.08);
    text-shadow: 0 0 6px rgba(255, 209, 102, 0.45);
    vertical-align: middle;
  }

  .worktree-ref {
    color: var(--wm-amber);
    font-weight: 700;
  }

  header small {
    color: var(--wm-muted);
    font-family: var(--wm-mono);
    font-size: 11px;
    margin-left: 6px;
  }

  .thread-meta span,
  .thread-message span,
  .thread-message code {
    padding: 2px 5px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-muted);
    font: 10px/1.2 var(--wm-mono);
  }

  .thread-message {
    margin: 8px 0;
    padding: 8px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
  }

  .thread-message.agent {
    border-color: var(--wm-purple);
    background: rgba(138, 109, 240, 0.06);
    box-shadow: 0 0 10px rgba(201, 167, 255, 0.12);
  }

  .thread-message div {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
    margin-bottom: 6px;
  }

  .thread-message p {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .reply {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    align-items: start;
  }

  .reply textarea,
  .inline-composer textarea {
    min-height: 70px;
  }

  .inline-composer {
    margin: 8px 10px 12px 64px;
    padding: 12px;
    border: 1px solid var(--wm-green);
    background: var(--wm-surface);
    box-shadow: 0 0 22px rgba(124, 255, 158, 0.14);
  }

  .inline-composer-header {
    display: flex;
    gap: 16px;
    align-items: start;
    justify-content: space-between;
    margin-bottom: 10px;
  }

  .inline-composer-header p {
    color: var(--wm-green);
    font-weight: 900;
    text-transform: uppercase;
  }

  .inline-composer-header h3 {
    margin: 0;
    font-size: 14px;
    line-height: 1.25;
    overflow-wrap: anywhere;
  }

  .inline-composer-header button {
    width: 34px;
    min-width: 34px;
  }

  .inline-composer textarea {
    min-height: 120px;
  }

  .composer-controls {
    display: grid;
    grid-template-columns: minmax(160px, 220px) minmax(140px, 180px);
  }

  .composer-actions {
    justify-content: flex-start;
  }

  code {
    color: var(--wm-amber);
    overflow-wrap: anywhere;
  }

  .message {
    color: var(--wm-amber);
    font-family: var(--wm-mono);
  }

  @media (max-width: 1240px) {
    .review {
      grid-template-columns: minmax(220px, 290px) 8px minmax(0, 1fr) !important;
    }

    .diff {
      grid-column: 3;
    }

    .diff {
      height: auto;
      min-height: 100vh;
    }
  }

  @media (max-width: 820px) {
    :global(body) {
      overflow: auto;
    }

    .review {
      display: block;
    }

    .files {
      position: static;
      height: auto;
      border: 0;
      border-bottom: 2px solid var(--wm-border-strong);
    }

    .resizer,
    .layout-controls {
      display: none;
    }

    .files {
      max-height: 40vh;
    }

    .diff {
      padding: 0 10px 10px;
      height: auto;
      min-height: 0;
    }

    .session-bar {
      margin: 0 -10px 10px;
    }

    h2 {
      position: sticky;
      top: 44px;
      background: var(--wm-surface);
      z-index: 1;
    }

    .inline-composer {
      margin-left: 10px;
    }

    .num {
      width: 42px;
      padding: 0 5px;
    }

    .code {
      padding: 0 6px;
      font-size: 11px;
    }
  }
</style>
