# OxiGeo Tile Server Dockerfile
# Multi-stage build for minimal final image

# Build stage
FROM rust:1.89 as builder

WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY benchmarks ./benchmarks

# Build the server (release mode)
RUN cargo build --release -p oxigeo-server

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 oxigeo

# Create directories
RUN mkdir -p /data /etc/oxigeo /var/cache/oxigeo && \
    chown -R oxigeo:oxigeo /data /etc/oxigeo /var/cache/oxigeo

# Copy binary from builder
COPY --from=builder /build/target/release/oxigeo-server /usr/local/bin/

# Copy example config
COPY crates/oxigeo-server/examples/config.toml /etc/oxigeo/config.toml.example

# Switch to non-root user
USER oxigeo

# Expose default port
EXPOSE 8080

# Set environment variables
ENV OXIGEO_HOST=0.0.0.0
ENV OXIGEO_PORT=8080
ENV OXIGEO_LOG_LEVEL=info

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/oxigeo-server", "--help"] || exit 1

# Default command
CMD ["/usr/local/bin/oxigeo-server", "--config", "/etc/oxigeo/config.toml"]
