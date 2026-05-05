# discord-rs

discord-rs is a Discord bot written in Rust.

It uses the Twilight ecosystem for Discord integration, Redis and MongoDB for
storage, and Axum for lightweight HTTP endpoints. The codebase is
organized around feature slices that register slash commands, handle
interactions, and process Discord gateway events through a shared registry.

## Quick start

To run the bot with local services:

```bash
docker compose up
```

To run the bot with a local Rust toolchain:

```bash
cargo run
```

The main binary lives in `src/main.rs`. A separate healthcheck binary is
available at `src/bin/healthcheck.rs` for container and deployment probes.

## Configuration

Configuration is provided through environment variables. Start with
`.env.example` and set the values needed for your environment.

Required variables:

- `DISCORD_TOKEN`
- `GOOGLE_API_KEY`
- `AI_BASE_PROMPT`

Common optional variables:

- `APP_ENV`
- `REDIS_URL`
- `MONGO_URI`
- `MONGO_DATABASE`
- `MONGO_USERNAME`
- `MONGO_PASSWORD`

## Development

Useful commands:

```bash
cargo fmt -- --check
cargo check
cargo clippy --all-targets -- -D warnings
cargo test --all-features -- --test-threads 1
```

The repository targets Rust 2024.

## Code layout

Important directories:

- `src/features/` exposes the current feature-facing modules and registry
  surface used by the dispatcher.
- `src/slices/` contains the concrete feature slice implementations that own
  command registration and event handling for domains such as onboarding, AI,
  moderation, community roles, admin config, and Warframe.
- `src/dispatch.rs` routes interactions and gateway events into the shared
  feature registry.
- `src/bot/`, `src/context/`, `src/configs/`, `src/dbs/`, and
  `src/observability/` contain shared runtime setup, configuration, storage,
  and HTTP/metrics support.
- `src/services/` contains reusable domain and infrastructure services used by
  features and runtime code.
- `src/platform/` contains test support and platform-specific helpers.
- `src/commands/` and `src/events/` remain as compatibility or legacy modules
  during the transition to slice-based feature handling.
- `tests/` contains integration tests.
- `benches/` contains benchmark code.

## License

This project is available under the MIT License. See `LICENSE`.
