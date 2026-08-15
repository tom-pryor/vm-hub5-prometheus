FROM rust:1-slim-trixie AS builder
WORKDIR /build

# Cache dependency builds separately from application code.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo "fn main() {}" > src/main.rs \
    && echo "" > src/lib.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
COPY tests ./tests
RUN touch src/main.rs src/lib.rs \
    && cargo build --release

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/vm-prom /usr/local/bin/vm-prom

EXPOSE 9938
ENTRYPOINT ["/usr/local/bin/vm-prom"]
