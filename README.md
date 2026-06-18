# agnes-mcp

A [Model Context Protocol](https://modelcontextprotocol.io) server that exposes the **Agnes AI** free models — image recognition, text generation, text-to-image, image-to-image, text-to-video, image-to-video, and keyframe animation — to any MCP-compatible client (Claude Desktop, Cursor, etc.).

Built in Rust with [`rust-mcp-sdk`](https://github.com/rust-mcp-stack/rust-mcp-sdk), statically compilable, Docker-ready, and zero-warning clean.

## Features

| Tool | Description |
| --- | --- |
| `agnes_chat` | Text generation / chat completions with `agnes-2.0-flash` (system prompts, history, temperature). |
| `agnes_image_recognition` | Vision / image understanding — describe, analyze, and answer questions about images. Accepts URLs, local files, or base64. |
| `agnes_generate_image` | Text-to-image and image-to-image with `agnes-image-2.1-flash`. |
| `agnes_generate_video` | Text-to-video, image-to-video, multi-image, and keyframe animation with `agnes-video-v2.0` (async, with optional polling). |
| `agnes_video_status` | Poll or check the status of a video generation task. |
| `agnes_enhance_prompt` | Expand a simple idea into a rich, detailed generation prompt (for image or video). |
| `health_check` | Verify connectivity and authentication with the Agnes API. |

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
| Env vars | `AGNES_API_KEY` (or `AGNES_TOKEN`), `AGNES_BASE_URL`, `AGNES_MCP_HOST`, `AGNES_MCP_PORT`, `AGNES_MCP_TRANSPORT`, `AGNES_MCP_LOG_LEVEL` |
| TOML `[agnes]` | `base_url`, `api_key`, `request_timeout_secs`, `poll_interval_secs`, `poll_timeout_secs`, `output_dir` |
| TOML `[server]` | `name`, `host`, `port`, `transport_mode` |
| TOML `[logging]` | `level` |

See [`config.example.toml`](config.example.toml) for a fully commented example.

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
    chat.rs            agnes_chat
    image_recognition.rs  agnes_image_recognition
    image.rs           agnes_generate_image
    video.rs           agnes_generate_video + agnes_video_status
    prompt.rs          agnes_enhance_prompt
    health.rs          health_check
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
