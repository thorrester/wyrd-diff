const DEFAULT_HOST = '127.0.0.1';
const BASE_PORT = 8765;
const PORT_WINDOW = 10;
const STORAGE_KEY = 'wyrd-diff/api-base';
const token = 'dev-local-token';

let apiBase = `http://${DEFAULT_HOST}:${BASE_PORT}`;
let discovery: Promise<string> | null = null;

async function probe(base: string): Promise<boolean> {
  try {
    const response = await fetch(`${base}/health`, { method: 'GET' });
    return response.ok;
  } catch {
    return false;
  }
}

async function discoverApiBase(): Promise<string> {
  if (typeof window === 'undefined') return apiBase;
  const cached = window.localStorage.getItem(STORAGE_KEY);
  if (cached && (await probe(cached))) {
    apiBase = cached;
    return apiBase;
  }
  for (let offset = 0; offset < PORT_WINDOW; offset += 1) {
    const candidate = `http://${DEFAULT_HOST}:${BASE_PORT + offset}`;
    if (await probe(candidate)) {
      apiBase = candidate;
      window.localStorage.setItem(STORAGE_KEY, candidate);
      return apiBase;
    }
  }
  throw new Error(
    `Wyrd Diff bridge unreachable on ${DEFAULT_HOST}:${BASE_PORT}..${BASE_PORT + PORT_WINDOW - 1}. Is the app running?`
  );
}

export function ensureApiBase(): Promise<string> {
  if (!discovery) discovery = discoverApiBase();
  return discovery;
}

export function resetApiBaseDiscovery(): void {
  discovery = null;
  if (typeof window !== 'undefined') window.localStorage.removeItem(STORAGE_KEY);
}

export type ReviewSession = {
  id: string;
  repo_id: string;
  title: string;
  base_ref: string;
  head_ref: string;
  branch: string | null;
  base_sha: string;
  head_sha: string;
  status: string;
};

export type RepoRecord = {
  id: string;
  name: string;
  path: string;
};

export type AgentSessionRecord = {
  id: string;
  agent_session_id: string;
  agent_name: string;
  repo_id: string | null;
  branch: string | null;
  review_session_id: string | null;
  started_at: string;
  last_activity_at: string;
  ended_at: string | null;
};

export type OverviewEntry = {
  repo: RepoRecord;
  branch: string;
  session: ReviewSession;
  open_thread_count: number;
  pending_thread_count: number;
  agent_sessions: AgentSessionRecord[];
  updated_at: string;
};

export type ReviewDiffLine = {
  id: string;
  file_path: string;
  old_line: number | null;
  new_line: number | null;
  line_kind: string;
  content: string;
};

export type SourceContext = {
  file_path: string;
  diff_line_id: string | null;
  old_line: number | null;
  new_line: number | null;
  line_kind: string | null;
  content: string | null;
};

export type CommentRecord = {
  id: string;
  session_id: string;
  file_path: string;
  diff_line_id: string | null;
  old_line: number | null;
  new_line: number | null;
  range_start_old_line: number | null;
  range_start_new_line: number | null;
  range_end_old_line: number | null;
  range_end_new_line: number | null;
  selected_text: string | null;
  body: string;
  status: string;
  visibility: string;
};

export type ThreadMessageRecord = {
  id: string;
  thread_id: string;
  author_kind: string;
  author_name: string | null;
  message_type: string;
  body: string;
  status: string;
  visibility: string;
  fix_import_id: string | null;
  created_at: string;
  updated_at: string;
};

export type ReviewThreadRecord = {
  id: string;
  session_id: string;
  file_path: string;
  anchor_diff_line_id: string | null;
  old_line: number | null;
  new_line: number | null;
  range_start_old_line: number | null;
  range_start_new_line: number | null;
  range_end_old_line: number | null;
  range_end_new_line: number | null;
  selected_text: string | null;
  status: string;
  visibility: string;
  created_at: string;
  updated_at: string;
  last_delivered_message_id: string | null;
  messages: ThreadMessageRecord[];
};

export type NoteRecord = {
  id: string;
  repo_id: string | null;
  session_id: string | null;
  title: string;
  body: string;
  note_type: string;
  status: string;
  visibility: string;
  source_context: SourceContext | null;
};

export type DecisionRecord = {
  id: string;
  repo_id: string | null;
  session_id: string | null;
  title: string;
  context: string;
  decision: string;
  rationale: string;
  alternatives: string | null;
  consequences: string | null;
  status: string;
  visibility: string;
  source_context: SourceContext | null;
};

export type FeedbackBatchThread = {
  batch_id: string;
  thread_id: string;
  delivery_kind: string;
  message_ids: string[];
};

export type FeedbackBatch = {
  id: string;
  session_id: string;
  agent_session_id: string | null;
  status: string;
  payload: string;
  thread_count: number;
  created_at: string;
  delivered_at: string | null;
  threads: FeedbackBatchThread[];
};

export type FixImportRecord = {
  id: string;
  session_id: string;
  commit_sha: string;
  diff_patch: string;
  agent_name: string | null;
  response_text: string | null;
  tests_json: string | null;
  accepted: boolean;
  created_at: string;
};

export type AgentContext = {
  repo: {
    id: string;
    name: string;
    path: string;
  };
  session: ReviewSession;
  open_comments: CommentRecord[];
  open_threads: ReviewThreadRecord[];
  accepted_decisions: DecisionRecord[];
  agent_visible_notes: NoteRecord[];
};

export type ReviewHunk = {
  id: string;
  patch: string;
  lines: ReviewDiffLine[];
};

export type ReviewFileSummary = {
  id: string;
  session_id: string;
  path: string;
  old_path: string | null;
  status: string;
  additions: number;
  deletions: number;
  review_state: string;
};

export type ReviewFile = {
  file: ReviewFileSummary;
  hunks: ReviewHunk[];
};

type RequestOptions = {
  method?: string;
  body?: unknown;
  signal?: AbortSignal;
  timeoutMs?: number;
};

export class ApiAbortError extends Error {
  constructor(message = 'request aborted') {
    super(message);
    this.name = 'ApiAbortError';
  }
}

export class ApiTimeoutError extends Error {
  constructor(public timeoutMs: number) {
    super(`request timed out after ${timeoutMs}ms`);
    this.name = 'ApiTimeoutError';
  }
}

function linkSignals(external: AbortSignal | undefined, timeoutMs: number | undefined) {
  if (!external && !timeoutMs) return { signal: undefined as AbortSignal | undefined, cleanup: () => {} };
  const controller = new AbortController();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const onExternalAbort = () => controller.abort(external?.reason);
  if (external) {
    if (external.aborted) controller.abort(external.reason);
    else external.addEventListener('abort', onExternalAbort, { once: true });
  }
  if (timeoutMs && timeoutMs > 0) {
    timer = setTimeout(() => controller.abort(new ApiTimeoutError(timeoutMs)), timeoutMs);
  }
  return {
    signal: controller.signal,
    cleanup: () => {
      if (timer) clearTimeout(timer);
      if (external) external.removeEventListener('abort', onExternalAbort);
    }
  };
}

export async function api<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const base = await ensureApiBase();
  const { signal, cleanup } = linkSignals(options.signal, options.timeoutMs);
  try {
    const response = await fetch(`${base}${path}`, {
      method: options.method ?? 'GET',
      headers: {
        authorization: `Bearer ${token}`,
        'content-type': 'application/json'
      },
      body: options.body === undefined ? undefined : JSON.stringify(options.body),
      signal
    });
    if (!response.ok) {
      throw new Error(await response.text());
    }
    return (await response.json()) as T;
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') {
      const reason = signal?.reason;
      if (reason instanceof ApiTimeoutError) throw reason;
      throw new ApiAbortError(typeof reason === 'string' ? reason : 'request aborted');
    }
    throw err;
  } finally {
    cleanup();
  }
}

export type AgentConfigResult = {
  id: string;
  display: string;
  path: string;
  action: 'created' | 'updated' | 'unchanged';
};

export type ConfigureAgentsResponse = {
  url: string;
  results: AgentConfigResult[];
};

export type ConfigureAgentResponse = {
  url: string;
  result: AgentConfigResult;
};

export type BridgeStatus = {
  ok: boolean;
  mcp_url: string;
  version: string;
};

export type AgentConfigState = 'ok' | 'mismatch' | 'missing' | 'no_file';

export type AgentStatusEntry = {
  id: string;
  display: string;
  path: string;
  detected: boolean;
  state: AgentConfigState;
  configured_url: string | null;
  snippet: string;
};

export type AgentStatusResponse = {
  mcp_url: string;
  agents: AgentStatusEntry[];
};

export { apiBase, token };
