# Cloudflare SSL Integration Example

This guide shows how to set up miwidothttp with Cloudflare Origin CA certificates.

## Prerequisites

1. A domain managed by Cloudflare
2. Cloudflare account with API access

## Step 1: Create API Token

1. Log in to Cloudflare Dashboard
2. Go to My Profile → API Tokens
3. Click "Create Token"
4. Use template "Create Custom Token" with:
   - Permissions:
     - Zone → SSL and Certificates → Edit
     - Zone → Zone → Read
   - Zone Resources:
     - Include → Specific zone → Your domain

## Step 2: Configure miwidothttp

Create `config.toml`:

```toml
[server]
http_port = 80
https_port = 443
enable_https = true

[ssl]
# Enable automatic certificate generation
auto_cert = true
# List all domains you want certificates for
domains = [
    "example.com",
    "www.example.com",
    "*.example.com"
]

[cloudflare]
# Your API token from Step 1
api_token = "pJFz9Q3h_YOUR_TOKEN_HERE_x8Kv2mN4"
# Find this in Cloudflare dashboard → Overview → Zone ID
zone_id = "023e105f4ecef8ad9ca31a8372d0c353"

# Optional: Use API Key instead of token
# api_key = "YOUR_GLOBAL_API_KEY"
# email = "your-email@example.com"

[logging]
level = "info"
file = "/var/log/miwidothttp/server.log"
```

## Step 3: Run the Server

```bash
# Build and run
cargo build --release
sudo ./target/release/miwidothttp --config config.toml
```

The server will:
1. Connect to Cloudflare API using your credentials
2. Generate an Origin CA certificate for your domains
3. Save the certificate locally for reuse
4. Automatically renew before expiration
5. Start serving HTTPS traffic immediately

## Step 4: Configure Cloudflare SSL/TLS

In Cloudflare Dashboard:
1. Go to SSL/TLS → Overview
2. Set encryption mode to "Full (strict)"
3. Go to SSL/TLS → Origin Server
4. You'll see the certificate created by miwidothttp

## Testing

```bash
# Test HTTPS
curl https://yourdomain.com

# Check certificate
openssl s_client -connect yourdomain.com:443 -servername yourdomain.com
```

## Clustering with Cloudflare

For high availability, deploy multiple nodes:

### Node 1 (Primary)
```toml
[cluster]
enabled = true
node_id = "prod-node-1"
bind_addr = "0.0.0.0:7946"
advertise_addr = "10.0.1.10:7946"
join_nodes = []

[cluster.raft]
enabled = true
bind_addr = "0.0.0.0:8090"
```

### Node 2 (Secondary)
```toml
[cluster]
enabled = true
node_id = "prod-node-2"
bind_addr = "0.0.0.0:7946"
advertise_addr = "10.0.1.11:7946"
join_nodes = ["10.0.1.10:7946"]

[cluster.raft]
enabled = true
bind_addr = "0.0.0.0:8090"
```

All nodes will share the same Cloudflare certificates and automatically handle failover.

## Troubleshooting

### Certificate Generation Failed
- Check API token permissions
- Verify Zone ID is correct
- Ensure domains are active in Cloudflare

### SSL Not Working
- Check firewall allows port 443
- Verify Cloudflare SSL mode is "Full (strict)"
- Check server logs: `/var/log/miwidothttp/server.log`

### Cluster Issues
- Ensure nodes can reach each other on ports 7946 and 8090
- Check network connectivity between nodes
- Verify node IDs are unique