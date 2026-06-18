# Dockerfile for local development/testing (distroless runtime)
# Uses rustls (no OpenSSL dependency) for portable, static-friendly builds.

FROM rust:1.88 AS builder

WORKDIR /app

# Copy manifests and build files first for better layer caching.
COPY Cargo.toml Cargo.lock ./
COPY build.rs ./
COPY src ./src
COPY config.example.toml ./

# Build the release binary.
RUN cargo build --release --bin agnes-mcp

# Runtime: distroless (small, no shell).
FROM gcr.io/distroless/cc-debian12:latest

WORKDIR /app

# Copy the binary and a default config.
COPY --from=builder /app/target/release/agnes-mcp /app/agnes-mcp
COPY --from=builder /app/config.example.toml /app/config.toml

EXPOSE 8080

ENV RUST_LOG=info
ENV AGNES_MCP_HOST=0.0.0.0
ENV AGNES_MCP_PORT=8080
ENV AGNES_MCP_TRANSPORT=hybrid

USER 65534:65534

ENTRYPOINT ["/app/agnes-mcp"]
CMD ["serve", "--config", "/app/config.toml"]
