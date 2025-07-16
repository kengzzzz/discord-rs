# Discord Bot

A Discord bot written in Rust providing a handful of slash commands and
background tasks. The project uses the
[Twilight](https://twilight.rs/) ecosystem and can be developed entirely in a
container.

## Prerequisites

* [Docker](https://www.docker.com/) and
  [Docker Compose](https://docs.docker.com/compose/) – recommended way to run the
  bot.
* Rust toolchain – only required when running without Docker.

## Installation

1. Copy `.env.example` to `.env` and fill in your credentials.
2. Start the development stack:

   ```bash
   docker-compose up
   ```

   The bot will rebuild automatically whenever source files change. If you wish
   to run the binary directly use `cargo run` and ensure the environment
   variables below are exported.

## Usage

### Running with Docker

```bash
# build containers and start the stack
docker-compose up
```

### Running with the Rust toolchain

```bash
cargo run
```

For production builds you can create a release image:

```bash
docker build -f Dockerfile.production -t discord-bot .
docker run --env-file .env discord-bot
```

## Configuration

The bot is configured entirely through environment variables. The most important
ones are listed below; see `.env.example` for the full list.

| Variable | Description |
|----------|-------------|
| `APP_ENV` | Environment name (`development`, `production`, etc.). Set to `production` to skip DB setup. |
| `DISCORD_TOKEN` | Discord bot token. |
| `GOOGLE_API_KEY` | Google GenAI API key. |
| `AI_BASE_PROMPT` | Base system prompt for the AI. |
| `REDIS_URL` | Redis connection string. |
| `MONGO_URI` | MongoDB connection URI. |
| `MONGO_USERNAME` | MongoDB user name. |
| `MONGO_PASSWORD` | MongoDB password. |
| `MONGO_AUTH_SOURCE` | Authentication database (default `admin`). |
| `MONGO_DATABASE` | Database name (default `develop`). |
| `MONGO_MAX_POOL_SIZE` | MongoDB max pool size (default `30`). |
| `MONGO_MIN_POOL_SIZE` | MongoDB min pool size (default `10`). |
| `MONGO_SSL` | Enable TLS connections when `true`. |
| `MONGO_TLS_CA_FILE` | Path to CA certificate file. |
| `MONGO_TLS_CERT_KEY_FILE` | Path to client certificate/key file. |
| `MONGO_TLS_INSECURE` | Allow invalid certificates when `true`. |

The bot exposes a health check on port `8080` at `/healthz` and shuts down
gracefully on termination signals.

### MongoDB pre/post images

Change stream watchers require pre and post images on certain collections.
Enable them with:

```javascript
db.runCommand({ collMod: "channels", changeStreamPreAndPostImages: { enabled: true } })
db.runCommand({ collMod: "roles", changeStreamPreAndPostImages: { enabled: true } })
db.runCommand({ collMod: "quarantines", changeStreamPreAndPostImages: { enabled: true } })
db.runCommand({ collMod: "messages", changeStreamPreAndPostImages: { enabled: true } })
db.runCommand({ collMod: "ai_prompts", changeStreamPreAndPostImages: { enabled: true } })
```

## Project Structure

```
src/
  commands/       # slash command implementations
  configs/        # configuration modules
  context/        # shared application state
  events/         # Discord event handlers
  services/       # background tasks and utilities
tests/            # integration tests
scripts/          # helper scripts (e.g. MongoDB replica set init)
Dockerfile.*      # development and production images
```

The [`Context`](src/context/mod.rs) struct bundles shared services (HTTP client,
database connections, caches, etc.) and is passed around using
`Arc<Context>` to avoid global state.

## Contributing

Pull requests are welcome! Please ensure:

1. Code is formatted with `cargo fmt`.
2. `cargo clippy --all-targets -- -D warnings` runs without errors.
3. All tests pass with `cargo test --all-features`.

Issues and feature requests are also appreciated.

## License

This project is licensed under the [MIT License](LICENSE).
