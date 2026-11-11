# discord-rs

A Discord bot for moderation, AI chat and Warframe tools.

[Terms of Service](TERMS.md) · [Privacy Policy](PRIVACY.md)

## Features

- Spam detection, scam-image detection with OCR, and quarantine
- AI chat and translation of text and images into Thai, using Gemini
- Role picker, `/intro` onboarding, message relay between servers, and a voice channel log
- Warframe status card with world cycles, Steel Path, Circuit, Baro, Sortie and Nightwave
- Role pings for weekly resets, Baro's arrival and Cetus night
- `/warframe` commands for market prices, builds, Circuit and status
- `/admin config` panel to set channels, roles and features for each server

## Run locally

```sh
cp .env.example .env    # set DISCORD_TOKEN and GOOGLE_API_KEY
mkdir -p models
curl -fsSL -o models/text-detection.onnx https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.onnx
curl -fsSL -o models/text-recognition.onnx https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.onnx
set -a; . ./.env; set +a
cargo run --release
```

Turn on the Server Members and Message Content intents in the Developer Portal. In each server, the bot needs Manage Roles, Manage Messages and Kick Members, and its role must be above the roles it assigns. It also needs to view and send messages in the channels you set up.

## Configuration

| Variable | Default | Purpose |
|---|---|---|
| `DISCORD_TOKEN` | required | Bot token |
| `DATABASE_PATH` | `/data/bot.sqlite3` | SQLite file |
| `GOOGLE_API_KEY` | unset | Gemini API key. AI is off without it. |
| `AI_SEARCH_PROVIDER` / `AI_SEARCH_API_KEY` | unset | `exa` or `tavily` web search for AI answers |
| `AI_DAILY_LIMIT` | `100` | AI requests per member per day |
| `SCAM_DETECT_MODELS_DIR` | `/usr/share/ocrs` | Folder with the OCR models |

To read a secret from a file, set `<NAME>_FILE` to the file's path. `.env.example` and `src/config.rs` list every option.

## Development

```sh
cargo fmt -- --check
cargo clippy --all-targets --features migrate -- -D warnings
cargo test
```

## Docker

The image runs as uid 1000 with a read-only root filesystem, so mount a writable `/data` directory owned by uid 1000. Give the container a 512 MiB memory limit. The health check is at `:8080/healthz` and Prometheus metrics are at `:8080/metrics`. Shutdown can take up to 27 s, so set `stop_grace_period: 30s` in Compose.

## License

MIT, see `LICENSE`.
