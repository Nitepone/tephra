# Build
FROM rust:1.93-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked && cp target/release/tephra /tephra

# Run
FROM debian:bookworm-slim
# tzdata so the `timezone` setting (and Lua's os.date) works; ca-certificates for apps' HTTPS fetches.
RUN apt-get update && apt-get install -y --no-install-recommends tzdata ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /tephra /usr/local/bin/tephra
COPY apps ./apps
COPY tephra.example.toml ./tephra.toml
VOLUME /app/data
EXPOSE 8374
CMD ["tephra", "-c", "/app/tephra.toml"]
