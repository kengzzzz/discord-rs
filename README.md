# Discord Bot

This repository contains a simple Discord bot written in Rust.

## Requirements

- [Docker](https://www.docker.com/) and [Docker Compose](https://docs.docker.com/compose/)
- Rust toolchain (only required when running the bot without Docker)

## Setup

1. Copy `.env.example` to `.env`.
2. Fill in the environment variables in the created file. Set `APP_ENV=production` in the production file.

### Environment Variables

- The following variables are used by the bot (see `.env.example`):

- `APP_ENV` – Name of the environment. Set to `production` to skip database setup commands.
- `DISCORD_TOKEN` – Discord bot token.
- `GOOGLE_API_KEY` – Google GenAI API key.
- `AI_BASE_PROMPT` – Base system prompt for the AI.
- `REDIS_URL` – Redis connection string.
- `MONGO_URI` – MongoDB connection URI.
- `MONGO_USERNAME` – MongoDB user name.
- `MONGO_PASSWORD` – MongoDB password.
- `MONGO_AUTH_SOURCE` – Authentication database (defaults to `admin`).
- `MONGO_DATABASE` – Database name (defaults to `develop`).
- `MONGO_MAX_POOL_SIZE` – MongoDB max pool size (defaults to `30`).
- `MONGO_MIN_POOL_SIZE` – MongoDB min pool size (defaults to `10`).
- `MONGO_SSL` – Enable TLS connections when set to `true`.
- `MONGO_TLS_CA_FILE` – Path to CA certificate file.
- `MONGO_TLS_CERT_KEY_FILE` – Path to client certificate and key file.
- `MONGO_TLS_INSECURE` – Allow invalid certificates when set to `true`.
- The bot exposes a health check on port `8080` at `/healthz`.
- The health endpoint reports unhealthy if the Discord connection drops or
  MongoDB stops responding.
- The bot listens for termination signals and shuts down gracefully.

### MongoDB Pre/Post Images

Change stream watchers require pre and post images to be enabled on these
collections: `channels`, `roles`, `quarantines`, `messages` and
`ai_prompts`. Enable them with:

```javascript
db.runCommand({ collMod: "channels", changeStreamPreAndPostImages: { enabled: true } })
db.runCommand({ collMod: "roles", changeStreamPreAndPostImages: { enabled: true } })
db.runCommand({ collMod: "quarantines", changeStreamPreAndPostImages: { enabled: true } })
db.runCommand({ collMod: "messages", changeStreamPreAndPostImages: { enabled: true } })
db.runCommand({ collMod: "ai_prompts", changeStreamPreAndPostImages: { enabled: true } })
```

## Development

The project provides a `docker-compose.yml` that starts MongoDB, Redis and the bot in watch mode.

```bash
# build containers and start the stack
docker-compose up
```

The bot source is mounted into the container and `cargo watch -x run` keeps it running whenever files change.

## Production

To build a production image and run it:

```bash
# build the production image
docker build -f Dockerfile.production -t discord-bot .

# run using the production environment file
docker run --env-file .env discord-bot
```

Alternatively you can build and run the binary directly with the Rust toolchain:

```bash
cargo build --release
./target/release/discord-bot
```

Make sure the required environment variables are available in your shell when running the binary directly.

## License

This project is licensed under the [MIT License](LICENSE).