# Discord Bot

A Discord bot written in Rust providing a handful of slash commands and
background tasks. The project uses the
[Twilight](https://twilight.rs/) ecosystem and can be developed entirely in a
container.

## Architecture Overview
The bot runs a single Twilight shard and processes events through priority queues backed by Tokio channels. A small Axum server exposes health and metrics endpoints.

## Folder Structure
```
src/               # core bot code
  bot/             # queue logic and worker tasks
  commands/        # slash command handlers
  configs/         # environment variable loaders
  context/         # shared resources (DB, HTTP, cache)
  observability/   # metrics and HTTP server
  services/        # background services
scripts/           # helper scripts
benches/           # benchmark harnesses
tests/             # integration tests
```

## Concurrency & Backpressure Model
Event dispatch uses three bounded mpsc queues (high, normal, low) paired with
semaphores that limit concurrent processing. Producers block on enqueue when the
corresponding queue is full. Workers measure how long events wait in queues and
how long enqueue operations block, recording both via Prometheus histograms.

## Metrics & Monitoring
| name | type | unit | labels | description | alert hint |
|------|------|------|--------|-------------|------------|
| `bot_events_total` | counter | events | priority, event_type, result | Number of events processed or enqueued | Sudden drops may indicate disconnects |
| `bot_handler_errors_total` | counter | errors | priority, event_type, result | Errors from event handlers | Watch for sustained increases |
| `bot_queue_wait_ms` | histogram | milliseconds | priority | Time events spend waiting in queue (residency) | Long tails show queue saturation |
| `bot_queue_enqueue_block_ms` | histogram | milliseconds | priority | Time producers block when queues are full | Spikes imply backpressure |
| `bot_interaction_ack_ms` | histogram | milliseconds | kind | Time from receive to initial interaction acknowledgement | High values hurt UX |

Low-cardinality labels avoid per-user or per-message dimensions to keep metric
cardinality manageable.

## /healthz and /metrics
The Axum server listens on port 8080 and exposes:
- `/healthz` – lightweight health indicator
- `/metrics` – Prometheus scrape endpoint

## Configuration
All settings come from environment variables, optionally loaded from a `.env`
file.

| key | type | default | required | description |
|-----|------|---------|----------|-------------|
| `APP_ENV` | string | `development` | no | Environment name |
| `DISCORD_TOKEN` | string | N/A | yes | Discord bot token |
| `GOOGLE_API_KEY` | string | N/A | yes | Google GenAI API key |
| `AI_BASE_PROMPT` | string | N/A | yes | AI system prompt |
| `REDIS_URL` | string | `redis://redis:6379` | no | Redis connection URL |
| `MONGO_URI` | string | `mongodb://mongo1:27017,mongo2:27017,mongo3:27017/?replicaSet=rs0` | no | MongoDB URI |
| `MONGO_USERNAME` | string | `homestead` | no | MongoDB username |
| `MONGO_PASSWORD` | string | `secret` | no | MongoDB password |
| `MONGO_AUTH_SOURCE` | string | `admin` | no | Mongo auth database |
| `MONGO_DATABASE` | string | `develop` | no | Mongo database name |
| `MONGO_MAX_POOL_SIZE` | number | `30` | no | Mongo connection pool max |
| `MONGO_MIN_POOL_SIZE` | number | `10` | no | Mongo connection pool min |
| `MONGO_SSL` | bool | `false` | no | Enable TLS to Mongo |
| `MONGO_TLS_CA_FILE` | string | none | no | Path to CA file |
| `MONGO_TLS_CERT_KEY_FILE` | string | none | no | Path to client cert/key |
| `MONGO_TLS_INSECURE` | bool | none | no | Accept invalid certificates |

## Local Setup & Running
```bash
# with Docker
docker-compose up

# using local Rust toolchain
cargo run
```

## Observing & Debugging
`RUST_LOG` controls logging verbosity. Metrics can be scraped from `/metrics`
and visualized with Prometheus and Grafana. Future considerations include adding
a readiness probe separate from `/healthz`.

## Interpreting Latencies
- **queue_wait_ms** measures how long an event sat in a queue before a worker
  picked it up (queue residency).
- **enqueue_block_ms** tracks how long senders waited when queues were full
  (producer backpressure).
- **interaction_ack_ms** records time from event receipt to initial interaction
  response, indicating perceived responsiveness.

## Development & Contribution
Pull requests are welcome! Please ensure:

1. Code is formatted with `cargo fmt`.
2. `cargo clippy --all-targets -- -D warnings` runs without errors.
3. All tests pass with `cargo test --all-features -- --test-threads 1`.

Issues and feature requests are also appreciated.

## License
Licensed under the [MIT License](LICENSE).
