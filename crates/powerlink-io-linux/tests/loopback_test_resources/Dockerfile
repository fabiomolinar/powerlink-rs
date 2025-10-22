# --- Builder Stage ---
# Use the official nightly Rust image based on Debian Bookworm for 2024 edition support.
# This is more explicit than the generic 'nightly' tag.
# The --platform flag ensures we build for x86_64, even on ARM hosts (e.g., Apple M1/M2).
FROM --platform=linux/amd64 rustlang/rust:nightly-bookworm as builder

# Create a working directory.
WORKDIR /powerlink-test

# Copy over your Cargo manifest files.
COPY ./Cargo.toml ./Cargo.toml
COPY ./Cargo.lock ./Cargo.lock
# Copy the source code.
COPY ./crates ./crates

# Add the standard Linux x86_64 target to the toolchain to ensure cross-compilation.
RUN rustup target add x86_64-unknown-linux-gnu

# Build the test binary for the x86_64 Linux target without running it.
RUN cargo test -p powerlink-io-linux --test loopback_test --no-run --target x86_64-unknown-linux-gnu

# Find the compiled test binary and move it to a predictable path.
# This is more robust than using wildcards in the final COPY command.
RUN find ./target/x86_64-unknown-linux-gnu/debug/deps -name 'loopback_test-*' -type f -executable -exec mv {} /usr/local/bin/powerlink-loopback-test \;


# --- Final Stage ---
# Use a minimal Debian base image for a small and secure final image.
FROM --platform=linux/amd64 debian:bookworm-slim

# Install the pcap library, which is a runtime dependency for pnet.
RUN apt-get update && apt-get install -y libpcap-dev --no-install-recommends && rm -rf /var/lib/apt/lists/*

# Copy the compiled test binary from the builder stage.
COPY --from=builder /usr/local/bin/powerlink-loopback-test /usr/local/bin/powerlink-loopback-test

# Set the execute permission on the test binary.
RUN chmod +x /usr/local/bin/powerlink-loopback-test

# Set the entrypoint for the container.
ENTRYPOINT ["/usr/local/bin/powerlink-loopback-test"]

