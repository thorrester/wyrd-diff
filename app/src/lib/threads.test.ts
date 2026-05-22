import { describe, expect, it } from 'vitest';
import type { ReviewThreadRecord, ThreadMessageRecord } from './api';
import {
  groupThreadsByLine,
  isThreadAwaitingHuman,
  isThreadPending,
  lastAgentMessage,
  lineAnchorKey,
  messageTypeLabel,
  threadAnchorLabel,
  threadLineLabel
} from './threads';

function msg(overrides: Partial<ThreadMessageRecord> = {}): ThreadMessageRecord {
  return {
    id: overrides.id ?? 'm-1',
    thread_id: overrides.thread_id ?? 't-1',
    author_kind: overrides.author_kind ?? 'human',
    author_name: overrides.author_name ?? null,
    message_type: overrides.message_type ?? 'comment',
    body: overrides.body ?? '',
    status: overrides.status ?? 'open',
    visibility: overrides.visibility ?? 'agent',
    fix_import_id: overrides.fix_import_id ?? null,
    created_at: overrides.created_at ?? '2025-01-01T00:00:00Z',
    updated_at: overrides.updated_at ?? '2025-01-01T00:00:00Z'
  };
}

function thread(overrides: Partial<ReviewThreadRecord> = {}): ReviewThreadRecord {
  return {
    id: overrides.id ?? 't-1',
    session_id: overrides.session_id ?? 's-1',
    file_path: overrides.file_path ?? 'src/lib.rs',
    anchor_diff_line_id: overrides.anchor_diff_line_id ?? null,
    old_line: overrides.old_line ?? null,
    new_line: overrides.new_line ?? null,
    range_start_old_line: overrides.range_start_old_line ?? null,
    range_start_new_line: overrides.range_start_new_line ?? null,
    range_end_old_line: overrides.range_end_old_line ?? null,
    range_end_new_line: overrides.range_end_new_line ?? null,
    selected_text: overrides.selected_text ?? null,
    status: overrides.status ?? 'open',
    visibility: overrides.visibility ?? 'agent',
    created_at: overrides.created_at ?? '2025-01-01T00:00:00Z',
    updated_at: overrides.updated_at ?? '2025-01-01T00:00:00Z',
    last_delivered_message_id: overrides.last_delivered_message_id ?? null,
    messages: overrides.messages ?? []
  };
}

describe('isThreadAwaitingHuman', () => {
  it('returns false when thread is resolved', () => {
    const t = thread({
      status: 'resolved',
      messages: [msg({ author_kind: 'agent' })]
    });
    expect(isThreadAwaitingHuman(t)).toBe(false);
  });

  it('returns false when there are no visible messages', () => {
    const t = thread({ messages: [] });
    expect(isThreadAwaitingHuman(t)).toBe(false);
  });

  it('ignores private messages when determining last author', () => {
    const t = thread({
      messages: [
        msg({ id: 'm1', author_kind: 'human' }),
        msg({ id: 'm2', author_kind: 'agent', visibility: 'private' })
      ]
    });
    expect(isThreadAwaitingHuman(t)).toBe(false);
  });

  it('returns true when last visible message is from agent', () => {
    const t = thread({
      messages: [msg({ id: 'm1', author_kind: 'human' }), msg({ id: 'm2', author_kind: 'agent' })]
    });
    expect(isThreadAwaitingHuman(t)).toBe(true);
  });

  it('returns false when last visible message is human', () => {
    const t = thread({
      messages: [msg({ id: 'm1', author_kind: 'agent' }), msg({ id: 'm2', author_kind: 'human' })]
    });
    expect(isThreadAwaitingHuman(t)).toBe(false);
  });
});

describe('isThreadPending', () => {
  it('returns false when resolved', () => {
    const t = thread({
      status: 'resolved',
      messages: [msg({ author_kind: 'human' })]
    });
    expect(isThreadPending(t)).toBe(false);
  });

  it('returns true when there is no watermark and a human message exists', () => {
    const t = thread({
      messages: [msg({ id: 'm1', author_kind: 'human' })]
    });
    expect(isThreadPending(t)).toBe(true);
  });

  it('returns false when only agent messages exist and no watermark', () => {
    const t = thread({
      messages: [msg({ id: 'm1', author_kind: 'agent' })]
    });
    expect(isThreadPending(t)).toBe(false);
  });

  it('returns true when a human message exists after the watermark', () => {
    const t = thread({
      last_delivered_message_id: 'm1',
      messages: [msg({ id: 'm1', author_kind: 'agent' }), msg({ id: 'm2', author_kind: 'human' })]
    });
    expect(isThreadPending(t)).toBe(true);
  });

  it('returns false when no human message exists after the watermark', () => {
    const t = thread({
      last_delivered_message_id: 'm2',
      messages: [msg({ id: 'm1', author_kind: 'human' }), msg({ id: 'm2', author_kind: 'agent' })]
    });
    expect(isThreadPending(t)).toBe(false);
  });

  it('falls back to global scan when watermark id is missing from messages', () => {
    const t = thread({
      last_delivered_message_id: 'missing',
      messages: [msg({ id: 'm1', author_kind: 'human' })]
    });
    expect(isThreadPending(t)).toBe(true);
  });

  it('ignores private messages', () => {
    const t = thread({
      messages: [msg({ id: 'm1', author_kind: 'human', visibility: 'private' })]
    });
    expect(isThreadPending(t)).toBe(false);
  });
});

describe('lastAgentMessage', () => {
  it('returns the most recent non-human visible message', () => {
    const t = thread({
      messages: [
        msg({ id: 'm1', author_kind: 'agent', body: 'first' }),
        msg({ id: 'm2', author_kind: 'agent', body: 'second' }),
        msg({ id: 'm3', author_kind: 'human', body: 'reply' })
      ]
    });
    expect(lastAgentMessage(t)?.id).toBe('m2');
  });

  it('returns null when only human messages exist', () => {
    const t = thread({ messages: [msg({ author_kind: 'human' })] });
    expect(lastAgentMessage(t)).toBeNull();
  });

  it('skips private agent messages', () => {
    const t = thread({
      messages: [
        msg({ id: 'm1', author_kind: 'agent', visibility: 'private' }),
        msg({ id: 'm2', author_kind: 'human' })
      ]
    });
    expect(lastAgentMessage(t)).toBeNull();
  });
});

describe('threadLineLabel', () => {
  it('returns empty string when no line anchors', () => {
    expect(threadLineLabel(thread())).toBe('');
  });

  it('uses new_line when set', () => {
    expect(threadLineLabel(thread({ new_line: 12 }))).toBe(':12');
  });

  it('falls back to old_line when new_line absent', () => {
    expect(threadLineLabel(thread({ old_line: 7 }))).toBe(':7');
  });

  it('formats a range', () => {
    expect(
      threadLineLabel(thread({ range_start_new_line: 10, range_end_new_line: 14, new_line: 10 }))
    ).toBe(':10-14');
  });

  it('collapses identical start/end into single line', () => {
    expect(threadLineLabel(thread({ range_start_new_line: 5, range_end_new_line: 5 }))).toBe(':5');
  });
});

describe('threadAnchorLabel', () => {
  it('returns empty for undefined thread', () => {
    expect(threadAnchorLabel(undefined)).toBe('');
  });

  it('returns file path alone when no line', () => {
    expect(threadAnchorLabel(thread({ file_path: 'a/b.rs' }))).toBe('a/b.rs');
  });

  it('appends new_line when present', () => {
    expect(threadAnchorLabel(thread({ file_path: 'a/b.rs', new_line: 9 }))).toBe('a/b.rs:9');
  });

  it('falls back to old_line', () => {
    expect(threadAnchorLabel(thread({ file_path: 'a/b.rs', old_line: 3 }))).toBe('a/b.rs:3');
  });
});

describe('messageTypeLabel', () => {
  it('title-cases each underscore-separated segment', () => {
    expect(messageTypeLabel('fix_proposal')).toBe('Fix Proposal');
    expect(messageTypeLabel('comment')).toBe('Comment');
    expect(messageTypeLabel('agent_question_v2')).toBe('Agent Question V2');
  });

  it('returns empty string for empty input', () => {
    expect(messageTypeLabel('')).toBe('');
  });
});

describe('lineAnchorKey', () => {
  it('encodes null lines as empty', () => {
    expect(lineAnchorKey('a.rs', null, null)).toBe('a.rs||');
  });

  it('encodes both lines', () => {
    expect(lineAnchorKey('a.rs', 1, 2)).toBe('a.rs|1|2');
  });
});

describe('groupThreadsByLine', () => {
  it('groups threads sharing a file+line anchor', () => {
    const a = thread({ id: 't1', file_path: 'a.rs', new_line: 10 });
    const b = thread({ id: 't2', file_path: 'a.rs', new_line: 10 });
    const c = thread({ id: 't3', file_path: 'a.rs', new_line: 20 });
    const groups = groupThreadsByLine([a, b, c]);
    expect(groups.size).toBe(2);
    expect(groups.get(lineAnchorKey('a.rs', null, 10))?.map((t) => t.id)).toEqual(['t1', 't2']);
    expect(groups.get(lineAnchorKey('a.rs', null, 20))?.map((t) => t.id)).toEqual(['t3']);
  });

  it('separates threads in different files even at same line', () => {
    const a = thread({ id: 't1', file_path: 'a.rs', new_line: 10 });
    const b = thread({ id: 't2', file_path: 'b.rs', new_line: 10 });
    const groups = groupThreadsByLine([a, b]);
    expect(groups.size).toBe(2);
  });
});
