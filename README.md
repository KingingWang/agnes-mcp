# agnes-mcp

A [Model Context Protocol](https://modelcontextprotocol.io) server that exposes the **Agnes AI** free models — image recognition, text-to-image, image-to-image, text-to-video, image-to-video, and keyframe animation — to any MCP-compatible client (Claude Desktop, Cursor, etc.).

Built in Rust with [`rust-mcp-sdk`](https://github.com/rust-mcp-stack/rust-mcp-sdk), statically compilable, Docker-ready, and zero-warning clean.

## Features

| Tool | Description |
| --- | --- |
| `agnes_image_recognition` | Vision / image understanding — describe, analyze, and answer questions about images. Accepts URLs, local files, or base64. Optional `detail`: `low` / `high` / `auto`. |
| `agnes_generate_image` | Text-to-image and image-to-image with `agnes-image-2.1-flash`. Optional `enhance_prompt` (expand prompt before generation) and `save_to` (download to local path). |
| `agnes_generate_video` | Text-to-video, image-to-video, multi-image, and keyframe animation with `agnes-video-v2.0` (async, with optional polling). Optional `enhance_prompt` and `save_to`. |
| `agnes_video_status` | Poll or check the status of a video generation task. Optional `save_to` to download once complete. |

> **Note:** `health_check` and `agnes_enhance_prompt` are no longer registered as MCP tools. Operators can still check connectivity via the `agnes-mcp health` CLI command. AI agents diagnose service issues from tool-call errors directly.

## Quick start

### 1. Configure

Generate a config file (or copy `config.example.toml`):

```bash
./agnes-mcp config --output config.toml
```

Then edit `config.toml` and set your Agnes API key, or provide it via the `AGNES_API_KEY` environment variable (recommended):

```bash
export AGNES_API_KEY="sk-..."
```

### 2. Run (stdio — for MCP clients)

```bash
./agnes-mcp serve --config config.toml --mode stdio
```

### 3. Run (HTTP/SSE — for network access)

```bash
./agnes-mcp serve --config config.toml --mode hybrid --host 0.0.0.0 --port 8080
```

Transport modes: `stdio`, `http`, `sse`, `hybrid`.

### Connect from an MCP client

Example Claude Desktop / Cursor `mcp.json` entry:

```json
{
  "mcpServers": {
    "agnes": {
      "command": "/path/to/agnes-mcp",
      "args": ["serve", "--config", "/path/to/config.toml", "--mode", "stdio"],
      "env": {
        "AGNES_API_KEY": "sk-..."
      }
    }
  }
}
```

## Configuration

`agnes-mcp` reads configuration in priority order: CLI args > environment variables > TOML file > defaults.

| Source | Keys |
| --- | --- |
| Env vars | `AGNES_API_KEY` (or `AGNES_TOKEN`), `AGNES_BASE_URL`, `AGNES_MODEL_TEXT`, `AGNES_MODEL_IMAGE`, `AGNES_MODEL_VIDEO`, `AGNES_MCP_HOST`, `AGNES_MCP_PORT`, `AGNES_MCP_TRANSPORT`, `AGNES_MCP_LOG_LEVEL` |
| TOML `[agnes]` | `base_url`, `api_key`, `model_text`, `model_image`, `model_video`, `request_timeout_secs`, `poll_interval_secs`, `poll_timeout_secs` |
| TOML `[server]` | `name`, `host`, `port`, `transport_mode` |
| TOML `[logging]` | `level` |

See [`config.example.toml`](config.example.toml) for a fully commented example.

## Tool parameters

### Configurable models

The Agnes model identifiers can be overridden via the `[agnes]` TOML section (`model_text`, `model_image`, `model_video`), the `AGNES_MODEL_TEXT` / `AGNES_MODEL_IMAGE` / `AGNES_MODEL_VIDEO` environment variables, or the `--model-text` / `--model-image` / `--model-video` CLI flags. Defaults match the Agnes free-tier models (`agnes-2.0-flash`, `agnes-image-2.1-flash`, `agnes-video-v2.0`).

### `enhance_prompt` (image / video generation)

Both `agnes_generate_image` and `agnes_generate_video` accept an optional `enhance_prompt` boolean (default `false`). When `true`, the chat model first expands the prompt into a rich, detailed generation prompt (subject + scene + style + lighting + composition + quality) before generation. This **adds one extra chat-model round trip (~1–5s)** — leave it `false` if your prompt is already detailed. On enhancement failure, generation falls back to the original prompt and a warning is appended to the output. When successful, the enhanced prompt is echoed for observability.

### `save_to` (image / video download)

`agnes_generate_image`, `agnes_generate_video`, and `agnes_video_status` accept an optional `save_to` path. When set, the generated asset is downloaded to that local path (a directory for multiple images, or a file path for a single file). For video generation, downloading only happens when the task has completed (`wait=true` on `agnes_generate_video`, or via `agnes_video_status` polling). Download failures are reported in the output but do not suppress the returned URL.

## CLI

```bash
agnes-mcp serve    # start the MCP server
agnes-mcp health   # check Agnes API connectivity
agnes-mcp config   # generate an example configuration file
```

Run `agnes-mcp serve --help` for all flags.

## Build

```bash
cargo build --release
```

Requirements: Rust 1.74+ (edition 2021). The project uses `rustls` (no OpenSSL), so static/musl builds work without any system TLS dependency.

### Static (musl) build

```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

## Docker

```bash
# Standard (distroless runtime)
docker build -t agnes-mcp .

# Alpine (with shell, for debugging)
docker build -f Dockerfile.alpine -t agnes-mcp:alpine .

# Scratch (tiny static image)
docker build -f Dockerfile.scratch -t agnes-mcp:scratch .

docker run --rm -p 8080:8080 -e AGNES_API_KEY="sk-..." agnes-mcp
```

Or with Docker Compose:

```bash
AGNES_API_KEY="sk-..." docker compose up --build
```

## Project structure

```
src/
  main.rs              Entry point + CLI
  lib.rs               Library root, logging init
  cli/                 clap commands (serve / health / config)
  config/              TOML + env configuration
  error/               Error types
  server/              AgnesServer, MCP handler, transports
  tools/               MCP tools + shared Agnes HTTP client
    agnes_client.rs    Agnes API client (chat / image / video)
    image_recognition.rs  agnes_image_recognition
    image.rs           agnes_generate_image
    video.rs           agnes_generate_video + agnes_video_status
    prompt.rs          prompt enhancement helper (internal, not an MCP tool)
    health.rs          CLI health check (not an MCP tool)
  utils/               Input validation & image encoding helpers
```

## Quality gates

This project enforces zero warnings:

```bash
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-features --all-targets -- -D warnings
cargo test --all-features
```

## License

MIT
