# discord-rs

discord-rs is a Discord bot written in Rust.

It uses the Twilight ecosystem for Discord integration, Redis and MongoDB for
storage, and Axum for lightweight HTTP endpoints. The codebase is being moved
toward a feature-oriented layout, with new bot behavior living under
`src/features/`.

## Quick start

To run the bot with local services:

```bash
docker-compose up
```

To run the bot with a local Rust toolchain:

```bash
cargo run
```

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
cargo fmt
cargo check
cargo clippy --all-targets -- -D warnings
cargo test --all-features -- --test-threads 1
```

The repository targets Rust 2024.

## Code layout

Important directories:

- `src/features/` contains feature behavior and the central feature registry.
- `src/bot/`, `src/context/`, `src/dbs/`, and `src/observability/` contain
  shared runtime code.
- `src/commands/`, `src/events/`, and parts of `src/services/` remain as
  legacy or compatibility layers.
- `src/utils/` contains generic helpers.
- `tests/` contains integration tests.
- `benches/` contains benchmark code.

## License

This project is available under the MIT License. See `LICENSE`.
