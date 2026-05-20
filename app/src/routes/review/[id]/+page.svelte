<script lang="ts">
  import { page } from '$app/stores';
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
    token,
    type ReviewFile,
    type SourceContext,
    type ReviewSession,
    type ReviewDiffLine
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
  let selected: ReviewDiffLine | null = null;
  let rangeStart: ReviewDiffLine | null = null;
  let rangeEnd: ReviewDiffLine | null = null;
  let selectedLines: ReviewDiffLine[] = [];
  let selectedLineIds = new Set<string>();
  let selectionLabel = 'Select a diff line.';
  let selectingRange = false;
  let commentBody = '';
  let noteTitle = '';
  let noteBody = '';
  let decisionTitle = '';
  let decisionContext = '';
  let decision = '';
  let rationale = '';
  let message = '';
  let filter = '';
  let leftWidth = 300;
  let rightWidth = 340;
  let filesHidden = false;
  let sideHidden = false;
  const collapsed = new Set<string>();
  const skipped = new Set<string>();
  const revealedLarge = new Set<string>();
  const largeChangeThreshold = 500;

  $: sessionId = $page.params.id;
  $: gridColumns = `${filesHidden ? 0 : leftWidth}px 8px minmax(0, 1fr) 8px ${sideHidden ? 0 : rightWidth}px`;
  $: visibleFiles = files.filter((item) =>
    item.file.path.toLowerCase().includes(filter.toLowerCase())
  );
  $: selectedLines = getSelectedRangeLines(files, selected, rangeStart, rangeEnd);
  $: selectedLineIds = new Set(selectedLines.map((line) => line.id));
  $: selectionLabel = formatRangeLabel(selectedLines);

  async function load() {
    const sessionData = await api<{ review_session: ReviewSession }>(
      `/api/review-sessions/${sessionId}`
    );
    const diffData = await api<{ files: ReviewFile[] }>(`/api/review-sessions/${sessionId}/diff`);
    session = sessionData.review_session;
    files = diffData.files;
  }

  function toggle(set: Set<string>, id: string) {
    if (set.has(id)) set.delete(id);
    else set.add(id);
    files = files;
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

  function selectedSourceContext(): SourceContext | null {
    const lines = selectedLines;
    if (lines.length === 0) return null;
    const first = lines[0];
    return {
      file_path: first.file_path,
      diff_line_id: first.id,
      old_line: first.old_line,
      new_line: first.new_line,
      line_kind: first.line_kind,
      content: lines.map((line) => line.content).join('\n')
    };
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

  function startResize(pane: 'files' | 'side', event: MouseEvent) {
    event.preventDefault();
    const startX = event.clientX;
    const startLeft = leftWidth;
    const startRight = rightWidth;

    function onMove(moveEvent: MouseEvent) {
      if (pane === 'files') {
        leftWidth = clamp(startLeft + moveEvent.clientX - startX, 180, 520);
      } else {
        rightWidth = clamp(startRight + startX - moveEvent.clientX, 260, 620);
      }
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
      window.removeEventListener('mouseup', onUp);
      document.body.classList.remove('selecting-range');
    }

    document.body.classList.add('selecting-range');
    window.addEventListener('mouseup', onUp);
  }

  function updateLineSelection(line: ReviewDiffLine) {
    if (!selectingRange || !rangeStart || line.file_path !== rangeStart.file_path) return;
    rangeEnd = line;
    selected = line;
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
      diff_line_id: first.id,
      old_line: first.old_line,
      new_line: first.new_line,
      range_start_old_line: first.old_line,
      range_start_new_line: first.new_line,
      range_end_old_line: last.old_line,
      range_end_new_line: last.new_line,
      selected_text: lines.map((line) => line.content).join('\n')
    };
  }

  async function saveComment() {
    const range = rangePayload();
    if (!range || !commentBody.trim()) return;
    await api(`/api/review-sessions/${sessionId}/comments`, {
      method: 'POST',
      body: {
        ...range,
        body: commentBody,
        status: 'open',
        visibility: 'agent'
      }
    });
    message = 'Comment saved.';
    commentBody = '';
    selected = null;
    rangeStart = null;
    rangeEnd = null;
  }

  async function saveNote() {
    await api(`/api/review-sessions/${sessionId}/notes`, {
      method: 'POST',
      body: {
        title: noteTitle,
        body: noteBody,
        note_type: 'thought',
        status: 'draft',
        visibility: 'agent',
        source_context: selectedSourceContext()
      }
    });
    noteTitle = '';
    noteBody = '';
    message = 'Note saved.';
  }

  async function saveDecision() {
    await api(`/api/review-sessions/${sessionId}/decisions`, {
      method: 'POST',
      body: {
        title: decisionTitle,
        context: decisionContext,
        decision,
        rationale,
        status: 'accepted',
        visibility: 'agent',
        source_context: selectedSourceContext()
      }
    });
    decisionTitle = '';
    decisionContext = '';
    decision = '';
    rationale = '';
    message = 'Decision saved.';
  }

  load().catch((error) => {
    message = error.message;
  });
</script>

<main
  class="review"
  class:files-hidden={filesHidden}
  class:side-hidden={sideHidden}
  style:grid-template-columns={gridColumns}
>
  <nav class="files" aria-hidden={filesHidden}>
    <a class="home" href="/">Wyrd Mind</a>
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
          document.getElementById(item.file.id)?.scrollIntoView({ block: 'start' });
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

  <button
    class="resizer left-resizer"
    aria-label="Resize file sidebar"
    on:mousedown={(event) => startResize('files', event)}
  ></button>

  <section class="diff">
    <div class="layout-controls">
      <button on:click={() => (filesHidden = !filesHidden)}>
        {filesHidden ? 'Show files' : 'Hide files'}
      </button>
      <button on:click={() => (sideHidden = !sideHidden)}>
        {sideHidden ? 'Show notes' : 'Hide notes'}
      </button>
      <a class="flow-link" href={`/trajectory/${sessionId}`}>Trajectory</a>
    </div>
    {#if session}
      <header>
        <h1>{session.title}</h1>
        <p>{session.base_ref} .. {session.head_ref}</p>
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
                      <td class="num">{line.old_line ?? ''}</td>
                      <td class="num">{line.new_line ?? ''}</td>
                      <!-- eslint-disable-next-line svelte/no-at-html-tags -- highlightedLine escapes raw text before injecting syntax spans -->
                      <td class="code">{@html highlightedLine(line)}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/each}
          {/if}
        </article>
      {/if}
    {/each}
  </section>

  <button
    class="resizer right-resizer"
    aria-label="Resize note sidebar"
    on:mousedown={(event) => startResize('side', event)}
  ></button>

  <aside class="side" aria-hidden={sideHidden}>
    <button class="pane-toggle" on:click={() => (sideHidden = true)}>Hide notes</button>
    <h2>Line comment</h2>
    <div class="target">
      {#if selectedLines.length > 0}
        {selectionLabel}
      {:else}
        Select a diff line.
      {/if}
    </div>
    <textarea bind:value={commentBody} placeholder="Comment"></textarea>
    <button on:click={saveComment}>Save comment</button>

    <h2>Thinking note</h2>
    <div class="context-preview">
      {#if selectedLines.length > 0}
        Capturing context: {selectionLabel}
        {#if selectedLines.length > 1}
          ({selectedLines.length} lines)
        {/if}
      {:else}
        No source line selected.
      {/if}
    </div>
    <input bind:value={noteTitle} placeholder="Title" />
    <textarea bind:value={noteBody} placeholder="What are you thinking?"></textarea>
    <button on:click={saveNote}>Save note</button>

    <h2>Decision</h2>
    <div class="context-preview">
      {#if selectedLines.length > 0}
        Capturing context: {selectionLabel}
        {#if selectedLines.length > 1}
          ({selectedLines.length} lines)
        {/if}
      {:else}
        No source line selected.
      {/if}
    </div>
    <input bind:value={decisionTitle} placeholder="Title" />
    <textarea bind:value={decisionContext} placeholder="Context"></textarea>
    <textarea bind:value={decision} placeholder="Decision"></textarea>
    <textarea bind:value={rationale} placeholder="Rationale"></textarea>
    <button on:click={saveDecision}>Save decision</button>

    <h2>Codex</h2>
    <p>Tell Codex: address all comments in session <code>{sessionId}</code>.</p>
    <p>Token: <code>{token}</code></p>
    {#if message}<p class="message">{message}</p>{/if}
  </aside>
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

  .review {
    display: grid;
    min-height: 100vh;
    background: var(--wm-bg);
    font-size: 13px;
  }

  .files,
  .side {
    position: sticky;
    top: 0;
    height: 100vh;
    overflow: auto;
    background: var(--wm-surface);
    padding: 14px;
  }

  .files {
    grid-column: 1;
    border-right: 2px solid var(--wm-border-strong);
  }

  .side {
    grid-column: 5;
    border-left: 2px solid var(--wm-border-strong);
  }

  .review.files-hidden .files,
  .review.side-hidden .side {
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
    border-left: 2px solid var(--wm-border);
    border-right: 2px solid var(--wm-border);
    background: var(--wm-black);
    box-shadow: none;
    cursor: col-resize;
  }

  .resizer:hover {
    background: var(--wm-green);
    box-shadow: none;
    transform: none;
  }

  .left-resizer {
    grid-column: 2;
  }

  .right-resizer {
    grid-column: 4;
  }

  .pane-toggle {
    width: 100%;
    margin: 0 0 10px;
  }

  .home {
    display: block;
    margin-bottom: 14px;
    padding: 9px 10px;
    border: 2px solid var(--wm-green);
    color: var(--wm-ink);
    font-weight: 900;
    font-size: 18px;
    line-height: 1;
    text-decoration: none;
    box-shadow: 4px 4px 0 var(--wm-black);
  }

  input,
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
  }

  .files button:hover {
    box-shadow: 3px 3px 0 var(--wm-black);
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
    grid-column: 3;
    min-width: 0;
    height: 100vh;
    padding: 16px;
    overflow: auto;
    background: linear-gradient(rgba(124, 255, 158, 0.025) 1px, transparent 1px), var(--wm-bg);
    background-size: 100% 34px;
  }

  .layout-controls {
    display: flex;
    gap: 8px;
    justify-content: flex-start;
    align-items: center;
    margin: 0 0 12px;
    padding: 5px;
    border: 2px solid var(--wm-border);
    background: var(--wm-surface);
  }

  .layout-controls button,
  .flow-link {
    min-height: 24px;
    padding: 4px 7px;
    border: 2px solid var(--wm-border-strong);
    background: var(--wm-ink);
    color: var(--wm-black);
    box-shadow: 2px 2px 0 var(--wm-black);
    font-size: 11px;
    font-weight: 800;
    line-height: 1;
    text-decoration: none;
    white-space: nowrap;
  }

  .layout-controls button:hover,
  .flow-link:hover {
    box-shadow: 3px 3px 0 var(--wm-black);
  }

  header,
  article {
    margin-bottom: 16px;
    border: 2px solid var(--wm-border-strong);
    background: var(--wm-surface);
    box-shadow: 4px 4px 0 var(--wm-black);
  }

  header {
    padding: 14px;
    border-color: var(--wm-green);
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
    border-bottom: 2px solid var(--wm-border);
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
    border-top: 2px dashed var(--wm-border);
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
  }

  tr.del {
    background: var(--wm-red-bg);
  }

  tr.selected {
    background: var(--wm-amber-bg);
    outline: 2px solid var(--wm-amber);
    outline-offset: -2px;
  }

  tr.range-selected {
    background: rgba(255, 209, 102, 0.24);
  }

  tr.range-selected td {
    border-top: 1px solid rgba(255, 209, 102, 0.38);
    border-bottom: 1px solid rgba(255, 209, 102, 0.38);
  }

  td {
    border-bottom: 1px solid #263036;
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
    width: 52px;
    padding: 0 8px;
    color: var(--wm-muted);
    text-align: right;
    user-select: none;
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

  .target {
    padding: 8px;
    border: 2px solid var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-muted);
    font: 12px/1.35 var(--wm-mono);
  }

  .context-preview {
    margin: 0 0 8px;
    padding: 7px 8px;
    border: 2px dashed var(--wm-border);
    color: var(--wm-muted);
    background: var(--wm-bg);
    font: 11px/1.35 var(--wm-mono);
    overflow-wrap: anywhere;
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

    .side {
      grid-column: 1 / -1;
      position: static;
      height: auto;
      border-top: 2px solid var(--wm-border-strong);
      border-left: 0;
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 14px;
      align-items: start;
    }

    .right-resizer {
      display: none;
    }

    .diff {
      grid-column: 3;
    }

    .side h2,
    .side .target,
    .side textarea,
    .side input,
    .side button,
    .side p {
      min-width: 0;
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

    .files,
    .side {
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

    .side {
      display: block;
    }

    .diff {
      padding: 10px;
      height: auto;
      min-height: 0;
    }

    h2 {
      position: sticky;
      top: 0;
      background: var(--wm-surface);
      z-index: 1;
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
