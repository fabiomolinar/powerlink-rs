# Stage 1: Build the test binary using the nightly Rust toolchain on Debian Bookworm
# Use the official rustlang/rust image for nightly on bookworm, specifying amd64 platform
FROM --platform=linux/amd64 rustlang/rust:nightly-bookworm as builder

# Install necessary build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    libpcap-dev \
    && rm -rf /var/lib/apt/lists/*

# Set the working directory inside the builder container
WORKDIR /powerlink-test

# Copy the entire workspace context
# This allows Cargo to cache dependencies effectively
COPY ../../../.. .

# Add the required target for cross-compilation (even if host is already x86_64)
RUN rustup target add x86_64-unknown-linux-gnu

# Compile the specific integration test binary for the target architecture.
# Using `cargo test --no-run` compiles the test executable without running it.
# We specify the package and the test name.
# Removed --ignored flag as it's not valid for `cargo test --no-run`
RUN cargo test \
    --package powerlink-rs-linux \
    --test loopback_test \
    --target x86_64-unknown-linux-gnu \
    --no-run

# Use find to locate the exact test binary and move it to a known location
# Cargo adds a hash to the test binary name in the deps directory.
RUN mkdir -p /app && \
    find target/x86_64-unknown-linux-gnu/debug/deps/ -maxdepth 1 -type f -name 'loopback_test-*' -executable -exec mv {} /app/powerlink-loopback-test \;


# Stage 2: Create the minimal final runtime image
# Use the slim debian image, specifying amd64 platform
FROM --platform=linux/amd64 debian:bookworm-slim

# Install only runtime dependencies (libpcap0.8 for pnet)
RUN apt-get update && apt-get install -y --no-install-recommends \
    libpcap0.8 \
    && rm -rf /var/lib/apt/lists/*

# Copy the built test binary from the builder stage's known location.
COPY --from=builder /app/powerlink-loopback-test /usr/local/bin/powerlink-loopback-test

# Set the execute permission on the test binary.
RUN chmod +x /usr/local/bin/powerlink-loopback-test

# Define the default command to run when the container starts.
# This can be overridden by docker-compose.
ENTRYPOINT ["/usr/local/bin/powerlink-loopback-test"]