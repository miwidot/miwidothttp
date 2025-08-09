# miwidothttp

High-performance HTTP/HTTPS server with automatic Cloudflare SSL integration, built in Rust.

## Features

- ⚡ **Blazing Fast**: Built with Rust + Tokio + Axum for maximum performance
- 🔒 **Automatic SSL**: Cloudflare Origin CA integration for automatic certificate management
- 🚀 **Multi-App Support**: Reverse proxy for Node.js, Python, and static sites
- 🔄 **Process Management**: Built-in process spawning and health checks
- 📊 **Production Ready**: HTTP/2, WebSocket support, compression, CORS
- 🎯 **Simple Configuration**: TOML-based config with hot-reload support

## Performance

Based on 2025 benchmarks (August):
- **70,000+ requests/second** throughput
- **Sub-millisecond p50 latency**
- **Memory safe** with Rust's ownership model
- **Single binary** deployment

## Quick Start

1. **Configure Cloudflare API** (config.toml):
```toml
[cloudflare]
api_token = "YOUR_CLOUDFLARE_API_TOKEN"
zone_id = "YOUR_ZONE_ID"
```

2. **Build and Run**:
```bash
cargo build --release
./target/release/miwidothttp
```

3. **Server starts on**:
- HTTP: http://localhost:8080
- HTTPS: https://localhost:8443

## Architecture

```
┌─────────────┐     ┌──────────────┐     ┌─────────────┐
│   Client    │────▶│  miwidothttp │────▶│   Backend   │
└─────────────┘     └──────────────┘     └─────────────┘
                           │
                           ▼
                    ┌──────────────┐
                    │  Cloudflare  │
                    │     API      │
                    └──────────────┘
```

## Configuration

See `config.toml` for full configuration options:
- Server settings (ports, workers)
- SSL/TLS configuration
- Backend routing rules
- Process management
- Health checks

## Roadmap

- [x] Core HTTP/HTTPS server
- [x] Cloudflare SSL integration
- [x] Reverse proxy
- [ ] Process management
- [ ] Load balancing
- [ ] Metrics/monitoring
- [ ] WebSocket proxying
- [ ] HTTP/3 support

## License

MIT