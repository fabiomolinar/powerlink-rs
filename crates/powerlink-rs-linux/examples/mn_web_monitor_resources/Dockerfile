# Use the slim-bookworm, platform-specific image as requested
FROM --platform=linux/amd64 rust:slim-bookworm AS builder

# Install libpcap-dev for pnet
RUN apt-get update && apt-get install -y libpcap-dev

# Create a new, empty workspace
WORKDIR /app
RUN cargo new --bin powerlink-rs-linux-example
WORKDIR /app/powerlink-rs-linux-example

# Copy in the manifests
COPY Cargo.toml Cargo.toml
COPY crates/powerlink-rs/Cargo.toml crates/powerlink-rs/Cargo.toml
COPY crates/powerlink-rs-linux/Cargo.toml crates/powerlink-rs-linux/Cargo.toml
COPY crates/powerlink-rs-monitor/Cargo.toml crates/powerlink-rs-monitor/Cargo.toml
# Add other HALs if they become dependencies
COPY crates/powerlink-rs-windows/Cargo.toml crates/powerlink-rs-windows/Cargo.toml
COPY crates/powerlink-rs-embedded/Cargo.toml crates/powerlink-rs-embedded/Cargo.toml

# Copy in the lock file
COPY Cargo.lock Cargo.lock

# Copy in the source code
# (This copies all source code, leveraging layer caching)
COPY crates/powerlink-rs crates/powerlink-rs
COPY crates/powerlink-rs-linux crates/powerlink-rs-linux
COPY crates/powerlink-rs-monitor crates/powerlink-rs-monitor
# Add other HALs
COPY crates/powerlink-rs-windows crates/powerlink-rs-windows
COPY crates/powerlink-rs-embedded crates/powerlink-rs-embedded

# Build the specific example binary, enabling the 'pcap' feature
RUN cargo build --release --example mn_web_monitor --features powerlink-rs-linux/pcap

# --- Final Image ---
# Use a matching bookworm-slim base image
FROM debian:bookworm-slim

# Install runtime dependencies (libpcap0.8)
RUN apt-get update && apt-get install -y libpcap0.8 && rm -rf /var/lib/apt/lists/*

# Copy the built binary from the builder stage
COPY --from=builder /app/powerlink-rs-linux-example/target/release/examples/mn_web_monitor /usr/local/bin/mn_web_monitor

# Set the binary as the entrypoint
ENTRYPOINT ["/usr/local/bin/mn_web_monitor"]