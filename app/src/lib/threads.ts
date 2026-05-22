import type { ReviewThreadRecord, ThreadMessageRecord } from './api';

export function isThreadPending(thread: ReviewThreadRecord): boolean {
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

export function isThreadAwaitingHuman(thread: ReviewThreadRecord): boolean {
  if (thread.status !== 'open') return false;
  const visible = thread.messages.filter((message) => message.visibility !== 'private');
  if (visible.length === 0) return false;
  return visible[visible.length - 1].author_kind !== 'human';
}

export function lastAgentMessage(thread: ReviewThreadRecord): ThreadMessageRecord | null {
  const visible = thread.messages.filter((message) => message.visibility !== 'private');
  for (let i = visible.length - 1; i >= 0; i -= 1) {
    if (visible[i].author_kind !== 'human') return visible[i];
  }
  return null;
}

export function threadLineLabel(thread: ReviewThreadRecord): string {
  const start =
    thread.range_start_new_line ??
    thread.new_line ??
    thread.range_start_old_line ??
    thread.old_line;
  const end = thread.range_end_new_line ?? thread.range_end_old_line ?? start;
  if (start == null) return '';
  if (end != null && end !== start) return `:${start}-${end}`;
  return `:${start}`;
}

export function threadAnchorLabel(thread: ReviewThreadRecord | undefined): string {
  if (!thread) return '';
  const line = thread.new_line ?? thread.old_line;
  return line == null ? thread.file_path : `${thread.file_path}:${line}`;
}

export function messageTypeLabel(type: string): string {
  return type
    .split('_')
    .map((part) => part.slice(0, 1).toUpperCase() + part.slice(1))
    .join(' ');
}

export function lineAnchorKey(
  filePath: string,
  oldLine: number | null,
  newLine: number | null
): string {
  return `${filePath}|${oldLine ?? ''}|${newLine ?? ''}`;
}

export function groupThreadsByLine(items: ReviewThreadRecord[]): Map<string, ReviewThreadRecord[]> {
  const map = new Map<string, ReviewThreadRecord[]>();
  for (const thread of items) {
    const key = lineAnchorKey(thread.file_path, thread.old_line, thread.new_line);
    const list = map.get(key);
    if (list) list.push(thread);
    else map.set(key, [thread]);
  }
  return map;
}
