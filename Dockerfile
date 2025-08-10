# Build stage
FROM rust:1.75 as builder

WORKDIR /app

# Copy manifest files
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Build release binary
RUN cargo build --release

# Runtime stage
FROM ubuntu:24.04

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    openssl \
    libssl3 \
    nodejs \
    python3 \
    php-fpm \
    && rm -rf /var/lib/apt/lists/*

# Create user for running the server
RUN useradd -r -s /bin/false miwidothttp

# Copy binary from builder
COPY --from=builder /app/target/release/miwidothttp /usr/local/bin/miwidothttp

# Create necessary directories
RUN mkdir -p /etc/miwidothttp /var/log/miwidothttp /var/lib/miwidothttp /app/static /app/certs \
    && chown -R miwidothttp:miwidothttp /etc/miwidothttp /var/log/miwidothttp /var/lib/miwidothttp /app

# Copy default configuration (optional)
COPY --chown=miwidothttp:miwidothttp config.toml /etc/miwidothttp/config.toml

WORKDIR /app

# Switch to non-root user
USER miwidothttp

# Expose ports
EXPOSE 8080 8443

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Default command
CMD ["/usr/local/bin/miwidothttp", "--config", "/etc/miwidothttp/config.toml"]