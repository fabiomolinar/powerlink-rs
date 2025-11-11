# Stage 1: Build the example binary using a stable Rust toolchain on Debian Bookworm
FROM --platform=linux/amd64 rust:slim-bookworm as builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    libpcap-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the entire workspace to leverage Docker layer caching
COPY ../../../.. .

# Build the specific example binary from the correct package
RUN cargo build --package powerlink-rs-linux --example io_module

# Stage 2: Create the minimal runtime image
FROM --platform=linux/amd64 debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    libpcap0.8 \
    && rm -rf /var/lib/apt/lists/*

# Copy the built binary from the builder stage
COPY --from=builder /app/target/debug/examples/io_module /usr/local/bin/powerlink-io-module

# Set execute permission
RUN chmod +x /usr/local/bin/powerlink-io-module

# The ENTRYPOINT will be the application binary
ENTRYPOINT ["/usr/local/bin/powerlink-io-module"]