# OxiGDAL Tile Server Dockerfile
# Multi-stage build for minimal final image

# Build stage
FROM rust:1.85 as builder

WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY benchmarks ./benchmarks

# Build the server (release mode)
RUN cargo build --release -p oxigdal-server

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 oxigdal

# Create directories
RUN mkdir -p /data /etc/oxigdal /var/cache/oxigdal && \
    chown -R oxigdal:oxigdal /data /etc/oxigdal /var/cache/oxigdal

# Copy binary from builder
COPY --from=builder /build/target/release/oxigdal-server /usr/local/bin/

# Copy example config
COPY crates/oxigdal-server/examples/config.toml /etc/oxigdal/config.toml.example

# Switch to non-root user
USER oxigdal

# Expose default port
EXPOSE 8080

# Set environment variables
ENV OXIGDAL_HOST=0.0.0.0
ENV OXIGDAL_PORT=8080
ENV OXIGDAL_LOG_LEVEL=info

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/oxigdal-server", "--help"] || exit 1

# Default command
CMD ["/usr/local/bin/oxigdal-server", "--config", "/etc/oxigdal/config.toml"]
