# agnes-mcp

A [Model Context Protocol](https://modelcontextprotocol.io) server that exposes the **Agnes AI** free models — image recognition, text-to-image, image-to-image, text-to-video, image-to-video, and keyframe animation — to any MCP-compatible client (Claude Desktop, Cursor, etc.).

Built in Rust with [`rust-mcp-sdk`](https://github.com/rust-mcp-stack/rust-mcp-sdk), statically compilable, Docker-ready, and zero-warning clean.

## Features

| Tool | Description |
| --- | --- |
| `agnes_image_recognition` | Vision / image understanding — describe, analyze, and answer questions about images. Accepts URLs, local files, or base64. Optional `detail`: `low` / `high` / `auto`. |
| `agnes_generate_image` | Text-to-image and image-to-image with `agnes-image-2.1-flash`. Reference images (`image_urls`) accept http(s) URLs, **local file paths**, `data:` URIs, or raw base64 (local files and base64 are encoded inline as `data:` URIs). Optional `enhance_prompt` (expand prompt before generation) and `save_to` (download to local path). |
| `agnes_generate_video` | Text-to-video, image-to-video, multi-image, and keyframe animation with `agnes-video-v2.0`. **Asynchronous only**: submits the task and returns a task id immediately. Optional `enhance_prompt`. Poll the result via `agnes_video_status`. |
| `agnes_video_status` | Check the status of a video generation task (single status check, no polling). Call periodically until the task reports `completed`. Optional `save_to` to download once complete. |

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

#### Multiple API keys (round-robin load balancing)

For higher throughput, you can supply multiple Agnes API keys. Requests are distributed evenly across all keys via a relaxed-atomic counter (no lock contention).

- **TOML**: `api_keys = ["sk-1", "sk-2", "sk-3"]` (under `[agnes]`)
- **Env**: `AGNES_API_KEYS="sk-1,sk-2,sk-3"` (comma-separated)
- **CLI**: `--api-key sk-1 --api-key sk-2 --api-key sk-3` (repeatable flag)

Keys from all sources are merged and deduped. The single-key form (`api_key`, `AGNES_API_KEY`) still works for backward compatibility; both forms can coexist.

#### Retry & cooldown (automatic key failover)

When an API key returns HTTP **429** (rate limited) or **401/403** (auth failure / revoked), it is automatically cooled down and the request is retried on the next healthy key:

- **429** → cooldown `key_rate_limit_cooldown_secs` (default **60s**)
- **401 / 403** → cooldown `key_cooldown_secs` (default **600s** = 10 min)

Other errors (network, 5xx, other 4xx, JSON parse) **fail fast** — they are not caused by the key, so retrying on a different key wouldn't help. Configure via the `[agnes]` TOML fields `key_cooldown_secs` / `key_rate_limit_cooldown_secs`, or the `AGNES_KEY_COOLDOWN_SECS` / `AGNES_KEY_RATE_LIMIT_COOLDOWN_SECS` env vars.

#### Video task affinity (important)

Agnes ties each video `task_id` to the API key that created it — querying with a different key is treated as a possible **key leak** by the server. This server pins every video status query to the key used at creation time. As a consequence:

- A video task created in **session A** (process) cannot be queried from **session B**. You'll see: `video task '...' was not created by this server session`.
- Video status queries are **not retried across keys**, even if the bound key is in cooldown — switching keys would break Agnes task ownership.
- Restart the server and previously-created task IDs become unqueryable (affinity is in-process only).

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
| Env vars | `AGNES_API_KEY` (or `AGNES_TOKEN`), `AGNES_API_KEYS`, `AGNES_BASE_URL`, `AGNES_MODEL_TEXT`, `AGNES_MODEL_IMAGE`, `AGNES_MODEL_VIDEO`, `AGNES_DISABLED_TOOLS`, `AGNES_KEY_COOLDOWN_SECS`, `AGNES_KEY_RATE_LIMIT_COOLDOWN_SECS`, `AGNES_MCP_HOST`, `AGNES_MCP_PORT`, `AGNES_MCP_TRANSPORT`, `AGNES_MCP_LOG_LEVEL` |
| TOML `[agnes]` | `base_url`, `api_key`, `api_keys`, `model_text`, `model_image`, `model_video`, `disabled_tools`, `request_timeout_secs`, `key_cooldown_secs`, `key_rate_limit_cooldown_secs` |
| TOML `[server]` | `name`, `host`, `port`, `transport_mode` |
| TOML `[logging]` | `level` |

See [`config.example.toml`](config.example.toml) for a fully commented example.

### Disabling tools

By default every built-in MCP tool is registered and exposed to clients. If you only
need a subset (e.g. you do not want to expose video generation), list the tool names
to disable in `[agnes].disabled_tools`. Names must match the canonical identifiers
exactly (case-sensitive); unknown names are ignored with a warning at startup.

Available tool names:

- `agnes_image_recognition`
- `agnes_generate_image`
- `agnes_generate_video`
- `agnes_video_status`

Configure via any of the three sources (entries from all sources are merged and
deduped):

- **TOML** (`[agnes]`):

  ```toml
  disabled_tools = ["agnes_generate_video", "agnes_video_status"]
  ```

- **Env**: `AGNES_DISABLED_TOOLS="agnes_generate_video,agnes_video_status"`
- **CLI**: `--disable-tool agnes_generate_video --disable-tool agnes_video_status`
  (repeatable flag).

Default is empty — no tools are disabled.

## Tool parameters

### Configurable models

The Agnes model identifiers can be overridden via the `[agnes]` TOML section (`model_text`, `model_image`, `model_video`), the `AGNES_MODEL_TEXT` / `AGNES_MODEL_IMAGE` / `AGNES_MODEL_VIDEO` environment variables, or the `--model-text` / `--model-image` / `--model-video` CLI flags. Defaults match the Agnes free-tier models (`agnes-2.0-flash`, `agnes-image-2.1-flash`, `agnes-video-v2.0`).

### `enhance_prompt` (image / video generation)

Both `agnes_generate_image` and `agnes_generate_video` accept an optional `enhance_prompt` boolean (default `false`). When `true`, the chat model first expands the prompt into a rich, detailed generation prompt (subject + scene + style + lighting + composition + quality) before generation. This **adds one extra chat-model round trip (~1–5s)** — leave it `false` if your prompt is already detailed. On enhancement failure, generation falls back to the original prompt and a warning is appended to the output. When successful, the enhanced prompt is echoed for observability.

### `image_urls` accepted formats (image-to-image)

`agnes_generate_image` accepts a list of reference images via `image_urls`. Each entry can be any of:

- **http(s) URL** — passed through unchanged: `https://example.com/ref.png`
- **Local file path** — the file is read and encoded as a `data:` URI inline (no public hosting required): `/path/to/ref.png`
- **`data:` URI** — passed through unchanged: `data:image/png;base64,...`
- **Raw base64 text** — wrapped as `data:image/png;base64,<input>`

This matches the Agnes API behavior, which supports `data:` URIs in `extra_body.image` for image-to-image generation. `agnes_image_recognition` also accepts the same set of input formats.

### `save_to` (image / video download)

`agnes_generate_image`, `agnes_generate_video`, and `agnes_video_status` accept an optional `save_to` path. When set, the generated asset is downloaded to that local path (a directory for multiple images, or a file path for a single file). `agnes_generate_video` is asynchronous-only and returns a task id without downloading; use `agnes_video_status` with `save_to` to download the video once the task reports `completed`. Download failures are reported in the output but do not suppress the returned URL.

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
