-- Drop the legacy line-comment table. v1 stores reviewer feedback as
-- review_threads + thread_messages. The `comments` table is no longer
-- written or read by any code path.
drop table if exists comments;
