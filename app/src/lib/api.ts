const apiBase = 'http://127.0.0.1:8765';
const token = 'dev-local-token';

export type ReviewSession = {
  id: string;
  repo_id: string;
  title: string;
  base_ref: string;
  head_ref: string;
  base_sha: string;
  head_sha: string;
  status: string;
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
  accepted_decisions: DecisionRecord[];
  agent_visible_notes: NoteRecord[];
};

export type ReviewHunk = {
  id: string;
  patch: string;
  lines: ReviewDiffLine[];
};

export type ReviewFile = {
  file: {
    id: string;
    path: string;
    old_path: string | null;
    status: string;
    additions: number;
    deletions: number;
    review_state: string;
  };
  hunks: ReviewHunk[];
};

type RequestOptions = {
  method?: string;
  body?: unknown;
};

export async function api<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const response = await fetch(`${apiBase}${path}`, {
    method: options.method ?? 'GET',
    headers: {
      authorization: `Bearer ${token}`,
      'content-type': 'application/json'
    },
    body: options.body === undefined ? undefined : JSON.stringify(options.body)
  });
  if (!response.ok) {
    throw new Error(await response.text());
  }
  return response.json() as Promise<T>;
}

export { apiBase, token };
