# Stage 1: Build the terrarium-owner binary
FROM rust:1.86-slim AS builder

RUN apt-get update && apt-get install -y musl-tools pkg-config && \
    rustup target add x86_64-unknown-linux-musl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --target x86_64-unknown-linux-musl -p terrarium-owner && \
    cp target/x86_64-unknown-linux-musl/release/terrarium-owner /terrarium-owner

# Stage 2: Slim runtime image
FROM ubuntu:22.04

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl git jq ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Install gh CLI
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
      -o /usr/share/keyrings/githubcli-archive-keyring.gpg && \
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
      > /etc/apt/sources.list.d/github-cli.list && \
    apt-get update && apt-get install -y gh && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /terrarium-owner /usr/local/bin/terrarium-owner

ENTRYPOINT ["terrarium-owner"]
