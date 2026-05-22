<script lang="ts">
  import { page } from '$app/stores';
  import { onDestroy, tick } from 'svelte';
  import { marked } from 'marked';
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
    ApiAbortError,
    ApiTimeoutError,
    api,
    apiBase,
    type FeedbackBatch,
    type ReviewFile,
    type ReviewFileSummary,
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
  let collapsedBatchThreads = new Set<string>();
  let hiddenBatchThreads = new Set<string>();
  let resolvingThreadIds = new Set<string>();
  let copiedThreadId = '';
  let copiedThreadTimer: ReturnType<typeof setTimeout> | undefined;
  let batchPanelEl: HTMLElement | undefined;
  let batchBannerVisible = false;
  let batchBannerTimer: ReturnType<typeof setTimeout> | undefined;
  let previewMode = new Set<string>();
  let collapsedFolders = new Set<string>();

  marked.setOptions({ gfm: true, breaks: false });
  marked.use({
    renderer: {
      code({ text, lang }: { text: string; lang?: string }) {
        const language = (lang ?? '').split(/\s/)[0];
        const supported = language && hljs.getLanguage(language);
        const html = supported
          ? hljs.highlight(text, { language, ignoreIllegals: true }).value
          : hljs.highlightAuto(text).value;
        const cls = supported ? `hljs language-${language}` : 'hljs';
        return `<pre><code class="${cls}">${html}</code></pre>`;
      }
    }
  });

  type TreeNode =
    | {
        kind: 'dir';
        name: string;
        path: string;
        children: TreeNode[];
        additions: number;
        deletions: number;
        fileCount: number;
      }
    | { kind: 'file'; name: string; path: string; item: ReviewFile };

  function buildTree(list: ReviewFile[]): TreeNode[] {
    type Dir = {
      name: string;
      path: string;
      dirs: Map<string, Dir>;
      files: ReviewFile[];
    };
    const root: Dir = { name: '', path: '', dirs: new Map(), files: [] };
    for (const item of list) {
      const segments = item.file.path.split('/');
      let cursor = root;
      for (let i = 0; i < segments.length - 1; i += 1) {
        const seg = segments[i];
        const dirPath = cursor.path ? `${cursor.path}/${seg}` : seg;
        let next = cursor.dirs.get(seg);
        if (!next) {
          next = { name: seg, path: dirPath, dirs: new Map(), files: [] };
          cursor.dirs.set(seg, next);
        }
        cursor = next;
      }
      cursor.files.push(item);
    }
    const sortDirs = (a: Dir, b: Dir) => a.name.localeCompare(b.name);
    const sortFiles = (a: ReviewFile, b: ReviewFile) => a.file.path.localeCompare(b.file.path);
    const collapseChain = (dir: Dir): Dir => {
      while (dir.dirs.size === 1 && dir.files.length === 0) {
        const [only] = dir.dirs.values();
        only.name = `${dir.name}/${only.name}`;
        only.path = only.path;
        return collapseChain(only);
      }
      return dir;
    };
    const toNodes = (dir: Dir): TreeNode[] => {
      const out: TreeNode[] = [];
      const childDirs = Array.from(dir.dirs.values()).sort(sortDirs);
      for (const child of childDirs) {
        const collapsed = collapseChain(child);
        const children = toNodes(collapsed);
        let additions = 0;
        let deletions = 0;
        let fileCount = 0;
        const visit = (nodes: TreeNode[]) => {
          for (const node of nodes) {
            if (node.kind === 'file') {
              additions += node.item.file.additions;
              deletions += node.item.file.deletions;
              fileCount += 1;
            } else {
              additions += node.additions;
              deletions += node.deletions;
              fileCount += node.fileCount;
            }
          }
        };
        visit(children);
        out.push({
          kind: 'dir',
          name: collapsed.name,
          path: collapsed.path,
          children,
          additions,
          deletions,
          fileCount
        });
      }
      for (const file of dir.files.slice().sort(sortFiles)) {
        const name = file.file.path.split('/').pop() ?? file.file.path;
        out.push({ kind: 'file', name, path: file.file.path, item: file });
      }
      return out;
    };
    return toNodes(root);
  }

  function toggleFolder(path: string) {
    if (collapsedFolders.has(path)) collapsedFolders.delete(path);
    else collapsedFolders.add(path);
    collapsedFolders = new Set(collapsedFolders);
  }

  function openFile(item: ReviewFile) {
    skipped.delete(item.file.id);
    collapsed.delete(item.file.id);
    files = files;
    ensureFileLoadedById(item.file.id);
    scrollDiffTo(item.file.id);
  }

  $: sessionId = $page.params.id;
  $: gridColumns = `${filesHidden ? 0 : leftWidth}px 8px minmax(0, 1fr)`;
  $: visibleFiles = files.filter((item) =>
    item.file.path.toLowerCase().includes(filter.toLowerCase())
  );
  $: fileTree = buildTree(visibleFiles);
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

  let routeController: AbortController | null = null;
  let loadGeneration = 0;

  function ignorableError(err: unknown): boolean {
    return err instanceof ApiAbortError || err instanceof ApiTimeoutError;
  }

  let loadedPaths = new Set<string>();
  const loadingPaths = new Set<string>();

  async function fetchFileDiff(path: string, signal: AbortSignal, generation: number) {
    if (loadedPaths.has(path) || loadingPaths.has(path)) return;
    loadingPaths.add(path);
    try {
      const response = await api<{ file: ReviewFile | null }>(
        `/api/review-sessions/${sessionId}/diff/file?path=${encodeURIComponent(path)}`,
        { signal, timeoutMs: 30_000 }
      );
      if (generation !== loadGeneration) return;
      if (!response.file) return;
      const idx = files.findIndex((f) => f.file.path === path);
      if (idx < 0) return;
      files[idx] = response.file;
      files = files;
      loadedPaths.add(path);
      loadedPaths = loadedPaths;
    } catch (error) {
      if (ignorableError(error)) return;
      console.error(`failed to load diff for ${path}`, error);
    } finally {
      loadingPaths.delete(path);
    }
  }

  async function load() {
    routeController?.abort();
    routeController = new AbortController();
    const signal = routeController.signal;
    const generation = ++loadGeneration;
    try {
      const sessionData = await api<{ review_session: ReviewSession }>(
        `/api/review-sessions/${sessionId}`,
        { signal, timeoutMs: 15_000 }
      );
      if (generation !== loadGeneration) return;
      const diffData = await api<{ files: ReviewFileSummary[] }>(
        `/api/review-sessions/${sessionId}/diff`,
        { signal, timeoutMs: 15_000 }
      );
      if (generation !== loadGeneration) return;
      const threadData = await api<{ threads: ReviewThreadRecord[] }>(
        `/api/review-sessions/${sessionId}/threads`,
        { signal, timeoutMs: 15_000 }
      );
      if (generation !== loadGeneration) return;
      session = sessionData.review_session;
      threads = threadData.threads;
      loadedPaths = new Set<string>();
      loadingPaths.clear();
      files = diffData.files.map((summary) => ({ file: summary, hunks: [] }));

      void hydrateFiles(signal, generation);
    } catch (error) {
      if (generation !== loadGeneration) return;
      if (ignorableError(error)) return;
      throw error;
    }
  }

  async function hydrateFiles(signal: AbortSignal, generation: number) {
    for (const item of files) {
      if (generation !== loadGeneration) return;
      if (skipped.has(item.file.id)) continue;
      if (isLargeFile(item) && !revealedLarge.has(item.file.id)) continue;
      await fetchFileDiff(item.file.path, signal, generation);
    }
  }

  function ensureFileLoadedById(fileId: string) {
    if (!routeController) return;
    const item = files.find((f) => f.file.id === fileId);
    if (!item) return;
    void fetchFileDiff(item.file.path, routeController.signal, loadGeneration);
  }

  onDestroy(() => {
    routeController?.abort();
  });

  function toggle(set: Set<string>, id: string) {
    if (set.has(id)) set.delete(id);
    else set.add(id);
    files = files;
  }

  function isMarkdownFile(path: string) {
    return /\.(md|mdx|markdown)$/i.test(path);
  }

  function reconstructMarkdown(item: ReviewFile): string {
    const lines: { n: number; text: string }[] = [];
    for (const hunk of item.hunks) {
      for (const line of hunk.lines) {
        if (line.new_line == null) continue;
        if (line.line_kind !== 'add' && line.line_kind !== 'ctx') continue;
        lines.push({ n: line.new_line, text: line.content.slice(1) });
      }
    }
    lines.sort((a, b) => a.n - b.n);
    return lines.map((entry) => entry.text).join('\n');
  }

  function renderMarkdown(item: ReviewFile): string {
    return marked.parse(reconstructMarkdown(item), { async: false }) as string;
  }

  function togglePreview(fileId: string) {
    if (previewMode.has(fileId)) previewMode.delete(fileId);
    else previewMode.add(fileId);
    previewMode = new Set(previewMode);
  }

  function scrollDiffTo(fileId: string) {
    requestAnimationFrame(() => {
      const target = document.getElementById(fileId);
      const container = document.querySelector<HTMLElement>('section.diff');
      if (!target || !container) return;
      const stickyOffset = container.querySelector<HTMLElement>('.session-bar')?.offsetHeight ?? 0;
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
    ensureFileLoadedById(id);
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

  function lineAnchorKey(filePath: string, oldLine: number | null, newLine: number | null): string {
    return `${filePath}|${oldLine ?? ''}|${newLine ?? ''}`;
  }

  function groupThreadsByLine(items: ReviewThreadRecord[]) {
    const map = new Map<string, ReviewThreadRecord[]>();
    for (const thread of items) {
      const key = lineAnchorKey(thread.file_path, thread.old_line, thread.new_line);
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
      collapsedBatchThreads = new Set();
      hiddenBatchThreads = new Set();
      flashBatchBanner();
      requestAnimationFrame(() => {
        batchPanelEl?.scrollIntoView({ behavior: 'smooth', block: 'center' });
      });
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

  function flashBatchBanner() {
    batchBannerVisible = true;
    if (batchBannerTimer) clearTimeout(batchBannerTimer);
    batchBannerTimer = setTimeout(() => (batchBannerVisible = false), 6000);
  }

  async function copyBatchPayload() {
    if (!lastBatch) return;
    await navigator.clipboard.writeText(lastBatch.payload);
    message = 'Markdown payload copied.';
  }

  function toggleBatchThread(threadId: string) {
    const next = new Set(collapsedBatchThreads);
    if (next.has(threadId)) next.delete(threadId);
    else next.add(threadId);
    collapsedBatchThreads = next;
  }

  function hideBatchThread(threadId: string) {
    const next = new Set(hiddenBatchThreads);
    next.add(threadId);
    hiddenBatchThreads = next;
  }

  function unhideAllBatchThreads() {
    hiddenBatchThreads = new Set();
  }

  async function resolveBatchThread(threadId: string) {
    const thread = threadById(threadId);
    if (!thread) {
      hideBatchThread(threadId);
      return;
    }
    if (resolvingThreadIds.has(threadId)) return;
    const next = new Set(resolvingThreadIds);
    next.add(threadId);
    resolvingThreadIds = next;
    try {
      await resolveThread(thread);
      hideBatchThread(threadId);
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
    } finally {
      const after = new Set(resolvingThreadIds);
      after.delete(threadId);
      resolvingThreadIds = after;
    }
  }

  function batchThreadSections(payload: string): string[] {
    const parts = payload.split(/\n---\n+/);
    return parts.slice(1).map((part) => part.trimStart());
  }

  function threadById(id: string): ReviewThreadRecord | undefined {
    return threads.find((thread) => thread.id === id);
  }

  function threadAnchorLabel(thread: ReviewThreadRecord | undefined): string {
    if (!thread) return '';
    const line = thread.new_line ?? thread.old_line;
    return line == null ? thread.file_path : `${thread.file_path}:${line}`;
  }

  async function copyBatchThread(threadId: string, section: string) {
    await navigator.clipboard.writeText(section);
    copiedThreadId = threadId;
    if (copiedThreadTimer) clearTimeout(copiedThreadTimer);
    copiedThreadTimer = setTimeout(() => (copiedThreadId = ''), 1500);
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
    {#snippet renderNodes(nodes: TreeNode[], depth: number)}
      {#each nodes as node (node.kind === 'dir' ? `d:${node.path}` : `f:${node.item.file.id}`)}
        {#if node.kind === 'dir'}
          {@const isOpen = filter ? true : !collapsedFolders.has(node.path)}
          <button
            class="tree-row tree-dir"
            class:open={isOpen}
            style:--depth={depth}
            on:click={() => toggleFolder(node.path)}
          >
            <span class="tree-label">
              <span class="tree-caret" aria-hidden="true">{isOpen ? '▾' : '▸'}</span>
              <span class="tree-name">{node.name}</span>
            </span>
            <small>
              <em class="tree-count">{node.fileCount}</em>
              <span class="adds">+{node.additions}</span>
              <span class="dels">-{node.deletions}</span>
            </small>
          </button>
          {#if isOpen}
            {@render renderNodes(node.children, depth + 1)}
          {/if}
        {:else}
          <button
            class="tree-row tree-file"
            class:skipped={skipped.has(node.item.file.id)}
            class:large={isLargeFile(node.item)}
            class:hidden-large={isLargeHidden(node.item)}
            style:--depth={depth}
            on:click={() => openFile(node.item)}
          >
            <span class="tree-label">
              <span class="tree-caret" aria-hidden="true"></span>
              <span class="tree-name">{node.name}</span>
            </span>
            <small>
              <span class="adds">+{node.item.file.additions}</span>
              <span class="dels">-{node.item.file.deletions}</span>
              {#if isLargeHidden(node.item)}
                <em>hidden</em>
              {/if}
            </small>
          </button>
        {/if}
      {/each}
    {/snippet}
    {@render renderNodes(fileTree, 0)}
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
      {#if lastBatch && !batchPanelOpen}
        <button
          class="dispatch-button"
          on:click={() => (batchPanelOpen = true)}
          title="Show last queued batch"
        >
          Show last batch
        </button>
      {/if}
    </div>
    {#if batchPanelOpen && lastBatch}
      <section class="batch-panel" bind:this={batchPanelEl}>
        <header>
          <div class="batch-meta">
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
          <div class="batch-actions">
            <button on:click={copyBatchPayload}>Copy full markdown</button>
            <button on:click={() => (batchPanelOpen = false)} aria-label="Close batch panel"
              >×</button
            >
          </div>
        </header>
        {#if batchBannerVisible}
          <p class="batch-banner">
            Batch queued. Agent must call
            <code>wyrd_diff.pending_feedback</code>
            with <code>session_id={sessionId}</code> to fetch.
          </p>
        {/if}
        {#if lastBatch.thread_count === 0}
          <p class="batch-empty">No open threads with new reviewer input.</p>
        {:else}
          {@const sections = batchThreadSections(lastBatch.payload)}
          {#if hiddenBatchThreads.size > 0}
            <p class="batch-hidden-note">
              {hiddenBatchThreads.size} hidden
              <button type="button" class="ghost" on:click={unhideAllBatchThreads}>show all</button>
            </p>
          {/if}
          <ul class="batch-threads">
            {#each lastBatch.threads as bt, idx (bt.thread_id)}
              {#if !hiddenBatchThreads.has(bt.thread_id)}
                {@const thread = threadById(bt.thread_id)}
                {@const section = sections[idx] ?? ''}
                {@const collapsed = collapsedBatchThreads.has(bt.thread_id)}
                {@const resolving = resolvingThreadIds.has(bt.thread_id)}
                <li class="batch-thread" class:collapsed>
                  <button
                    type="button"
                    class="batch-thread-head"
                    on:click={() => toggleBatchThread(bt.thread_id)}
                    aria-expanded={!collapsed}
                  >
                    <span class="caret">{collapsed ? '▶' : '▼'}</span>
                    <span class="batch-thread-title">
                      Thread {idx + 1} — {threadAnchorLabel(thread)}
                    </span>
                    <span class="batch-thread-meta">
                      <span class="delivery">{bt.delivery_kind}</span>
                      <span>{bt.message_ids.length} msg</span>
                    </span>
                  </button>
                  {#if !collapsed}
                    <div class="batch-thread-body">
                      <div class="batch-thread-actions">
                        <code class="thread-id">{bt.thread_id.slice(0, 8)}</code>
                        <button
                          type="button"
                          class="ghost"
                          on:click={() => copyBatchThread(bt.thread_id, section)}
                        >
                          {copiedThreadId === bt.thread_id ? 'copied' : 'copy thread'}
                        </button>
                        <button
                          type="button"
                          class="ghost"
                          disabled={resolving}
                          on:click={() => resolveBatchThread(bt.thread_id)}
                          title="Mark thread resolved and hide from this batch"
                        >
                          {resolving ? 'resolving…' : 'resolve'}
                        </button>
                        <button
                          type="button"
                          class="ghost"
                          on:click={() => hideBatchThread(bt.thread_id)}
                          title="Hide from this batch view (does not change thread status)"
                        >
                          hide
                        </button>
                      </div>
                      <pre>{section}</pre>
                    </div>
                  {/if}
                </li>
              {/if}
            {/each}
          </ul>
        {/if}
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
              {#if isMarkdownFile(item.file.path) && !isLargeHidden(item)}
                <button
                  class:active={previewMode.has(item.file.id)}
                  on:click={() => togglePreview(item.file.id)}
                  title="Render markdown from the head-side content"
                >
                  {previewMode.has(item.file.id) ? 'Show diff' : 'Preview MD'}
                </button>
              {/if}
              {#if isLargeHidden(item)}
                <button on:click={() => showLargeFile(item.file.id)}>Show diff</button>
              {:else}
                <button on:click={() => toggle(collapsed, item.file.id)}>
                  {collapsed.has(item.file.id) ? 'Expand' : 'Collapse'}
                </button>
              {/if}
              <button
                on:click={() => {
                  const wasSkipped = skipped.has(item.file.id);
                  toggle(skipped, item.file.id);
                  if (wasSkipped) ensureFileLoadedById(item.file.id);
                }}>Skip</button
              >
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
          {:else if !collapsed.has(item.file.id) && previewMode.has(item.file.id)}
            <!-- eslint-disable-next-line svelte/no-at-html-tags -- markdown is reconstructed from the head-side diff for a local repo -->
            <div class="md-preview">{@html renderMarkdown(item)}</div>
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
                    {#each threadsByLine.get(lineAnchorKey(line.file_path, line.old_line, line.new_line)) ?? [] as thread (thread.id)}
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

  .files .tree-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 6px;
    align-items: center;
    width: 100%;
    min-height: 24px;
    margin: 0;
    padding: 3px 6px 3px calc(6px + var(--depth, 0) * 12px);
    border: 0;
    background: transparent;
    color: var(--wm-ink);
    font-size: 12px;
    font-weight: 600;
    line-height: 1.2;
    box-shadow: none;
    text-align: left;
    cursor: pointer;
    transition: background 0.08s;
  }

  .files .tree-row:hover {
    background: rgba(124, 255, 158, 0.07);
    border-color: transparent;
    box-shadow: none;
    transform: none;
  }

  .files .tree-row:active {
    transform: none;
  }

  .files .tree-row.skipped {
    opacity: 0.45;
    text-decoration: line-through;
  }

  .files .tree-row.large .tree-name {
    border-bottom: 1px dashed var(--wm-border-strong);
  }

  .files .tree-row.hidden-large .tree-name {
    color: var(--wm-amber);
  }

  .files .tree-dir {
    font-weight: 800;
    color: var(--wm-muted);
    text-transform: none;
  }

  .files .tree-dir.open {
    color: var(--wm-ink);
  }

  .tree-label {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }

  .tree-caret {
    display: inline-block;
    width: 10px;
    color: var(--wm-subtle);
    font-size: 10px;
    text-align: center;
  }

  .tree-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .files small {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    color: var(--wm-muted);
    font-family: var(--wm-mono);
    font-size: 11px;
    white-space: nowrap;
  }

  .files .tree-count {
    padding: 0 4px;
    border: 1px solid var(--wm-border);
    color: var(--wm-subtle);
    font-style: normal;
    font-size: 10px;
  }

  .files .adds {
    color: var(--wm-green);
  }

  .files .dels {
    color: var(--wm-red);
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
    padding: 0 6px 16px 0;
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
    margin: 0 -6px 12px 0;
    padding: 10px 12px;
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

  .batch-banner {
    margin: 0 0 8px;
    padding: 8px 10px;
    border: 1px solid var(--wm-green);
    background: rgba(124, 255, 158, 0.06);
    color: var(--wm-ink);
    font-size: 12px;
  }

  .batch-banner code {
    padding: 1px 4px;
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
    color: var(--wm-green);
  }

  .batch-empty {
    margin: 0;
    color: var(--wm-muted);
    font-size: 12px;
  }

  .batch-hidden-note {
    margin: 0 0 6px;
    color: var(--wm-muted);
    font-size: 12px;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .batch-threads {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }

  .batch-thread {
    border: 1px solid var(--wm-border);
    background: var(--wm-bg);
  }

  .batch-thread-head {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 10px;
    border: 0;
    background: transparent;
    color: var(--wm-ink);
    font: 700 12px/1.2 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    cursor: pointer;
    text-align: left;
  }

  .batch-thread-head:hover {
    background: var(--wm-surface);
  }

  .caret {
    color: var(--wm-muted);
    font-size: 10px;
    width: 12px;
  }

  .batch-thread-title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .batch-thread-meta {
    display: flex;
    gap: 8px;
    color: var(--wm-muted);
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0;
  }

  .delivery {
    padding: 1px 5px;
    border: 1px solid var(--wm-border);
    color: var(--wm-amber);
  }

  .batch-thread-body {
    padding: 0 10px 10px;
    display: grid;
    gap: 6px;
  }

  .batch-thread-actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .thread-id {
    color: var(--wm-muted);
    font-size: 11px;
  }

  .batch-thread-body .ghost {
    padding: 3px 8px;
    background: transparent;
    border: 1px solid var(--wm-border-strong);
    color: var(--wm-ink);
    font: 700 10px/1.2 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    cursor: pointer;
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

  .md-preview {
    padding: 14px 18px;
    color: var(--wm-ink);
    font:
      14px/1.55 var(--wm-font),
      system-ui;
    background: var(--wm-bg);
    overflow-wrap: anywhere;
  }

  .md-preview :global(h1),
  .md-preview :global(h2),
  .md-preview :global(h3),
  .md-preview :global(h4) {
    margin: 18px 0 8px;
    color: var(--wm-green);
    text-shadow: 0 0 8px rgba(124, 255, 158, 0.25);
  }

  .md-preview :global(h1) {
    font-size: 22px;
  }
  .md-preview :global(h2) {
    font-size: 18px;
  }
  .md-preview :global(h3) {
    font-size: 15px;
  }

  .md-preview :global(p),
  .md-preview :global(ul),
  .md-preview :global(ol),
  .md-preview :global(blockquote),
  .md-preview :global(table) {
    margin: 8px 0;
  }

  .md-preview :global(ul),
  .md-preview :global(ol) {
    padding-left: 22px;
  }

  .md-preview :global(blockquote) {
    padding: 6px 12px;
    border-left: 3px solid var(--wm-amber);
    color: var(--wm-muted);
    background: rgba(255, 209, 102, 0.05);
  }

  .md-preview :global(code) {
    padding: 1px 5px;
    background: var(--wm-surface-2);
    border: 1px solid var(--wm-border);
    color: var(--wm-amber);
    font-family: var(--wm-mono);
    font-size: 12.5px;
  }

  .md-preview :global(pre) {
    padding: 10px 12px;
    background: var(--wm-black);
    border: 1px solid var(--wm-border);
    overflow-x: auto;
  }

  .md-preview :global(pre code) {
    padding: 0;
    background: transparent;
    border: 0;
    color: var(--wm-ink);
  }

  .md-preview :global(table) {
    width: auto;
    border-collapse: collapse;
  }

  .md-preview :global(th),
  .md-preview :global(td) {
    padding: 4px 10px;
    border: 1px solid var(--wm-border);
  }

  .md-preview :global(a) {
    color: var(--wm-blue);
    text-decoration: underline;
  }

  .md-preview :global(hr) {
    border: 0;
    border-top: 1px solid var(--wm-border);
    margin: 16px 0;
  }

  .md-preview :global(img) {
    max-width: 100%;
  }

  h2 button.active {
    border-color: var(--wm-amber);
    color: var(--wm-amber);
    box-shadow: var(--wm-glow-amber);
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
    width: 30px;
    padding: 0 4px;
    color: var(--wm-subtle);
    text-align: right;
    user-select: none;
  }

  .num.line-action {
    width: 26px;
    padding: 0 2px;
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
