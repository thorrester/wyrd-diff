create table if not exists schema_migrations (
  version integer primary key,
  applied_at text not null
);

create table if not exists repos (
  id text primary key,
  name text not null,
  path text not null unique,
  created_at text not null,
  updated_at text not null
);

create table if not exists review_sessions (
  id text primary key,
  repo_id text not null references repos(id),
  title text not null,
  base_ref text not null,
  head_ref text not null,
  base_sha text not null,
  head_sha text not null,
  status text not null,
  created_at text not null,
  updated_at text not null
);

create table if not exists active_review_sessions (
  repo_id text primary key references repos(id),
  session_id text not null references review_sessions(id),
  start_sha text not null,
  updated_at text not null
);

create table if not exists commits (
  id text primary key,
  session_id text not null references review_sessions(id),
  sha text not null,
  short_sha text not null,
  subject text not null,
  body text,
  author_name text,
  author_email text,
  authored_at text
);

create table if not exists files (
  id text primary key,
  session_id text not null references review_sessions(id),
  path text not null,
  old_path text,
  status text not null,
  additions integer not null,
  deletions integer not null,
  review_state text not null
);

create table if not exists hunks (
  id text primary key,
  file_id text not null references files(id),
  old_start integer,
  old_lines integer,
  new_start integer,
  new_lines integer,
  patch text not null
);

create table if not exists diff_lines (
  id text primary key,
  hunk_id text not null references hunks(id),
  file_path text not null,
  old_line integer,
  new_line integer,
  line_kind text not null,
  content text not null
);

create table if not exists comments (
  id text primary key,
  session_id text not null references review_sessions(id),
  file_path text not null,
  diff_line_id text references diff_lines(id),
  old_line integer,
  new_line integer,
  range_start_old_line integer,
  range_start_new_line integer,
  range_end_old_line integer,
  range_end_new_line integer,
  selected_text text,
  body text not null,
  status text not null,
  visibility text not null,
  created_at text not null,
  updated_at text not null
);

create table if not exists review_threads (
  id text primary key,
  session_id text not null references review_sessions(id),
  file_path text not null,
  anchor_diff_line_id text references diff_lines(id),
  old_line integer,
  new_line integer,
  range_start_old_line integer,
  range_start_new_line integer,
  range_end_old_line integer,
  range_end_new_line integer,
  selected_text text,
  status text not null,
  visibility text not null,
  created_at text not null,
  updated_at text not null
);

create table if not exists thread_messages (
  id text primary key,
  thread_id text not null references review_threads(id),
  author_kind text not null,
  author_name text,
  message_type text not null,
  body text not null,
  status text not null,
  visibility text not null,
  fix_import_id text references fix_imports(id),
  created_at text not null,
  updated_at text not null
);

create table if not exists notes (
  id text primary key,
  repo_id text references repos(id),
  session_id text references review_sessions(id),
  title text not null,
  body text not null,
  note_type text not null,
  status text not null,
  visibility text not null,
  source_file_path text,
  source_diff_line_id text references diff_lines(id),
  source_old_line integer,
  source_new_line integer,
  source_line_kind text,
  source_content text,
  created_at text not null,
  updated_at text not null
);

create table if not exists decisions (
  id text primary key,
  repo_id text references repos(id),
  session_id text references review_sessions(id),
  title text not null,
  context text not null,
  decision text not null,
  rationale text not null,
  alternatives text,
  consequences text,
  status text not null,
  visibility text not null,
  source_file_path text,
  source_diff_line_id text references diff_lines(id),
  source_old_line integer,
  source_new_line integer,
  source_line_kind text,
  source_content text,
  created_at text not null,
  updated_at text not null
);

create table if not exists trajectory_events (
  id text primary key,
  repo_id text references repos(id),
  session_id text references review_sessions(id),
  event_kind text not null,
  payload_json text not null,
  created_at text not null
);

create table if not exists agent_task_packets (
  id text primary key,
  session_id text not null references review_sessions(id),
  title text not null,
  payload_json text not null,
  status text not null,
  created_at text not null
);

create table if not exists fix_imports (
  id text primary key,
  session_id text not null references review_sessions(id),
  commit_sha text not null,
  diff_patch text not null,
  agent_name text,
  response_text text,
  tests_json text,
  accepted integer not null,
  created_at text not null
);

create virtual table if not exists search_index using fts5(
  record_type,
  record_id,
  title,
  body
);
