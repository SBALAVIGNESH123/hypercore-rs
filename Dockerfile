# Build stage
FROM rust:1.80-slim-bookworm AS builder

WORKDIR /usr/src/hypercore

# Install build dependencies for C++ and llama.cpp
RUN apt-get update && apt-get install -y \
    build-essential \
    cmake \
    libclang-dev \
    git \
    && rm -rf /var/lib/apt/lists/*

# Copy source code
COPY . .

# Build the release binary
RUN cargo build --release

# Production stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies (e.g., for SIMD/OpenMP if needed)
RUN apt-get update && apt-get install -y \
    libgomp1 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary from the builder
COPY --from=builder /usr/src/hypercore/target/release/hypercore-rs /usr/local/bin/hypercore

# Default configuration (can be overridden by volume mount)
COPY hypercore.yaml /app/hypercore.yaml

# Expose the API port (matches CLI default)
EXPOSE 8080

# Set entrypoint
ENTRYPOINT ["hypercore"]

# Default to serve mode, users can override args
CMD ["serve", "--model", "/app/models/model.gguf"]
