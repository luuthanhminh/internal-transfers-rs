# syntax=docker/dockerfile:1
# Base images are pinned so a rebuild produces the same toolchain and OS.
FROM rust:1.99.1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
# Cache mounts keep the crate registry and compiled dependencies between builds,
# so changing application code does not recompile every dependency.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
    && cp target/release/internal-transfers /usr/local/bin/internal-transfers

FROM debian:bookworm-slim
# No CA bundle needed: TLS to PostgreSQL uses rustls with bundled webpki roots.
RUN useradd --system --no-create-home --uid 10001 app
COPY --from=build /usr/local/bin/internal-transfers /usr/local/bin/internal-transfers
USER app
EXPOSE 8080
# Exec form: the binary is PID 1 and receives SIGTERM directly for graceful shutdown.
CMD ["/usr/local/bin/internal-transfers"]
