# Patched twilight-http-ratelimiting 0.18.0

Upstream source from crates.io with one change in `src/actor.rs`: the GC.

Upstream keeps rate-limit state for every path it has seen: a queue per top-level resource (one per
interaction token, `webhooks/<app>/<token>`) and a bucket row per full path (one per message id for
deletes and reactions). The GC runs every 6 h and only walks the bucket rows, so the per-token queue
is never freed (64-130 B per interaction for good) and everything else stays up to 6 h (~600 B per
interaction answered with an edit). Any member can drive that with commands.

The patched GC runs every 10 s, drops bucket rows whose queue has nothing in flight or pending,
then drops every queue with nothing in flight, pending or waiting on a reset, and shrinks both
tables. A path forgotten while its bucket is exhausted costs at most one 429 on its next use.

Test: `gc_keeps_requests_in_flight_and_pending` (requests in flight, pending and reused across a GC).
Remove this patch once upstream frees idle queues.
