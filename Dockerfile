FROM rust:1.81-bookworm

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        git ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Pre-cache dependencies
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src tests && \
    echo 'fn main() { println!("placeholder"); }' > src/main.rs && \
    echo '' > src/lib.rs && \
    cargo build --release && \
    rm -rf src tests

# Now copy the real source
COPY . .

# Build release binary
RUN cargo build --release && \
    cargo test --all --no-run

# Default to running the e2e
ENV XDG_DATA_HOME=/data
ENV PATH=/app/target/release:$PATH

CMD ["/app/scripts/run-e2e.sh"]
