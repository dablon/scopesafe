FROM rust:1.85-bookworm

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        git ca-certificates curl jq && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy only manifests first to maximize docker layer cache
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src tests && \
    echo 'fn main() { println!("placeholder"); }' > src/main.rs && \
    echo '' > src/lib.rs && \
    cargo build --release 2>&1 | tail -5 && \
    rm -rf src tests target/release/deps/scopesafe* target/release/scopesafe*

# Now copy the real source
COPY . .

# Build release binary + tests (tests need to be compiled before run-e2e)
RUN cargo build --release 2>&1 | tail -5 && \
    cargo test --all --no-run 2>&1 | tail -5

ENV XDG_DATA_HOME=/data
ENV PATH=/app/target/release:$PATH

CMD ["/app/scripts/run-e2e.sh"]
