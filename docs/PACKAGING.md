# Packaging & Static Site Deployment Guide

## Table of Contents
- [Static Site Packaging](#static-site-packaging)
- [Application Bundling](#application-bundling)
- [Distribution Packages](#distribution-packages)
- [Container Images](#container-images)
- [CI/CD Packaging](#cicd-packaging)

## Static Site Packaging

### Preparing Static Sites

#### Basic Static Site Structure
```
my-static-site/
├── index.html
├── css/
│   ├── main.css
│   └── responsive.css
├── js/
│   ├── app.js
│   └── vendor/
├── images/
│   ├── logo.png
│   └── backgrounds/
├── fonts/
│   └── custom.woff2
└── favicon.ico
```

#### Configuration for Static Hosting
```toml
# config-static.toml
[[vhosts]]
domains = ["static.example.com", "www.static.example.com"]
priority = 100
root = "/var/www/static-site"
index_files = ["index.html", "index.htm"]
directory_listing = false

[vhosts.cache]
enabled = true
ttl_seconds = 86400  # 24 hours for static assets

[vhosts.compression]
enabled = true
types = ["text/html", "text/css", "application/javascript", "image/svg+xml"]
level = 6

[vhosts.headers]
"Cache-Control" = "public, max-age=31536000"  # 1 year
"X-Content-Type-Options" = "nosniff"
```

### Building Optimized Static Packages

#### 1. HTML/CSS/JS Sites
```bash
#!/bin/bash
# build-static.sh

# Set variables
SITE_NAME="my-site"
BUILD_DIR="dist"
DEPLOY_DIR="/var/www/${SITE_NAME}"

# Clean build directory
rm -rf ${BUILD_DIR}
mkdir -p ${BUILD_DIR}

# Copy files
cp -r src/* ${BUILD_DIR}/

# Minify HTML
find ${BUILD_DIR} -name "*.html" -exec html-minifier \
  --collapse-whitespace \
  --remove-comments \
  --minify-css true \
  --minify-js true \
  -o {} {} \;

# Minify CSS
find ${BUILD_DIR} -name "*.css" -exec csso {} -o {} \;

# Minify JavaScript
find ${BUILD_DIR} -name "*.js" -exec terser {} -o {} -c -m \;

# Optimize images
find ${BUILD_DIR} -name "*.jpg" -o -name "*.jpeg" | xargs -I {} jpegoptim --strip-all {}
find ${BUILD_DIR} -name "*.png" | xargs -I {} optipng -o7 {}
find ${BUILD_DIR} -name "*.svg" | xargs -I {} svgo {}

# Generate cache manifest
find ${BUILD_DIR} -type f -exec md5sum {} \; > ${BUILD_DIR}/manifest.txt

# Create tarball
tar -czf ${SITE_NAME}.tar.gz -C ${BUILD_DIR} .

echo "Static site packaged: ${SITE_NAME}.tar.gz"
```

#### 2. React/Vue/Angular Apps
```bash
# React build
npm run build
# Output in build/

# Vue build
npm run build
# Output in dist/

# Angular build
ng build --prod
# Output in dist/

# Package for deployment
tar -czf app.tar.gz -C dist/ .
```

#### 3. Static Site Generators

**Jekyll**
```bash
bundle exec jekyll build
tar -czf jekyll-site.tar.gz -C _site/ .
```

**Hugo**
```bash
hugo --minify
tar -czf hugo-site.tar.gz -C public/ .
```

**Gatsby**
```bash
gatsby build
tar -czf gatsby-site.tar.gz -C public/ .
```

**Next.js Static Export**
```bash
npm run build
npm run export
tar -czf nextjs-site.tar.gz -C out/ .
```

### Deploying Static Sites

#### Direct Deployment Script
```bash
#!/bin/bash
# deploy-static.sh

PACKAGE="site.tar.gz"
DEPLOY_HOST="server.example.com"
DEPLOY_PATH="/var/www/html"
BACKUP_PATH="/var/backups/sites"

# Backup existing site
ssh ${DEPLOY_HOST} "
  if [ -d ${DEPLOY_PATH} ]; then
    tar -czf ${BACKUP_PATH}/backup-\$(date +%Y%m%d-%H%M%S).tar.gz -C ${DEPLOY_PATH} .
  fi
"

# Upload and extract new site
scp ${PACKAGE} ${DEPLOY_HOST}:/tmp/
ssh ${DEPLOY_HOST} "
  rm -rf ${DEPLOY_PATH}/*
  tar -xzf /tmp/${PACKAGE} -C ${DEPLOY_PATH}
  chown -R www-data:www-data ${DEPLOY_PATH}
  chmod -R 755 ${DEPLOY_PATH}
  rm /tmp/${PACKAGE}
"

# Reload server
ssh ${DEPLOY_HOST} "systemctl reload miwidothttp"

echo "Deployment complete!"
```

#### Zero-Downtime Deployment
```bash
#!/bin/bash
# zero-downtime-deploy.sh

SITE="mysite"
NEW_VERSION="v2.0.0"
DEPLOY_ROOT="/var/www"
CURRENT_LINK="${DEPLOY_ROOT}/${SITE}"
RELEASES_DIR="${DEPLOY_ROOT}/releases/${SITE}"

# Create release directory
RELEASE_DIR="${RELEASES_DIR}/${NEW_VERSION}"
mkdir -p ${RELEASE_DIR}

# Extract new version
tar -xzf ${SITE}.tar.gz -C ${RELEASE_DIR}

# Update symlink atomically
ln -sfn ${RELEASE_DIR} ${CURRENT_LINK}.tmp
mv -Tf ${CURRENT_LINK}.tmp ${CURRENT_LINK}

# Keep last 5 releases
cd ${RELEASES_DIR}
ls -t | tail -n +6 | xargs -r rm -rf

echo "Deployed ${NEW_VERSION} with zero downtime"
```

## Application Bundling

### PHP Application Packaging

#### WordPress Bundle
```bash
#!/bin/bash
# package-wordpress.sh

# Create bundle directory
mkdir -p wordpress-bundle/{app,config,data}

# Copy WordPress files
cp -r /path/to/wordpress/* wordpress-bundle/app/

# Add configuration
cat > wordpress-bundle/config/config.toml << EOF
[[vhosts]]
domains = ["wordpress.example.com"]
root = "/var/www/wordpress"

[vhosts.backend]
type = "phpfpm"
socket = "/run/php/php8.3-fpm.sock"

[vhosts.backend.env]
WP_HOME = "https://wordpress.example.com"
WP_SITEURL = "https://wordpress.example.com"
EOF

# Add PHP-FPM pool config
cat > wordpress-bundle/config/pool.conf << EOF
[wordpress]
user = www-data
group = www-data
listen = /run/php/wordpress.sock
pm = dynamic
pm.max_children = 50
pm.start_servers = 5
pm.min_spare_servers = 5
pm.max_spare_servers = 35
EOF

# Create deployment script
cat > wordpress-bundle/deploy.sh << 'EOF'
#!/bin/bash
cp -r app/* /var/www/wordpress/
cp config/pool.conf /etc/php/8.3/fpm/pool.d/wordpress.conf
systemctl reload php8.3-fpm
EOF

# Package
tar -czf wordpress-bundle.tar.gz wordpress-bundle/
```

#### Laravel Bundle
```bash
#!/bin/bash
# package-laravel.sh

# Build Laravel app
composer install --no-dev --optimize-autoloader
npm run production
php artisan config:cache
php artisan route:cache
php artisan view:cache

# Create bundle
tar -czf laravel-app.tar.gz \
  --exclude=node_modules \
  --exclude=.git \
  --exclude=.env \
  --exclude=storage/logs/* \
  --exclude=storage/framework/cache/* \
  .
```

### Node.js Application Packaging

#### Express App Bundle
```bash
#!/bin/bash
# package-node-app.sh

# Install production dependencies
npm ci --production

# Create bundle with dependencies
tar -czf node-app.tar.gz \
  --exclude=.git \
  --exclude=.env \
  --exclude=logs \
  --exclude=*.log \
  package.json \
  package-lock.json \
  node_modules/ \
  src/ \
  public/

# Create standalone executable (optional)
npm install -g pkg
pkg . --target node18-linux-x64 --output app-binary
```

#### PM2 Ecosystem Bundle
```bash
# ecosystem.config.js
module.exports = {
  apps: [{
    name: 'app',
    script: './src/index.js',
    instances: 'max',
    exec_mode: 'cluster',
    env: {
      NODE_ENV: 'production',
      PORT: 3000
    }
  }]
};

# Package with PM2 config
tar -czf pm2-app.tar.gz \
  ecosystem.config.js \
  package.json \
  src/
```

### Python Application Packaging

#### Django Bundle
```bash
#!/bin/bash
# package-django.sh

# Create virtual environment
python3 -m venv venv
source venv/bin/activate

# Install dependencies
pip install -r requirements.txt

# Collect static files
python manage.py collectstatic --noinput

# Create bundle
tar -czf django-app.tar.gz \
  --exclude=__pycache__ \
  --exclude=*.pyc \
  --exclude=.git \
  --exclude=media \
  --exclude=*.sqlite3 \
  .
```

#### Flask with Gunicorn
```bash
# Create requirements.txt
pip freeze > requirements.txt

# Gunicorn config
cat > gunicorn.conf.py << EOF
bind = "127.0.0.1:5000"
workers = 4
worker_class = "sync"
worker_connections = 1000
max_requests = 1000
max_requests_jitter = 50
EOF

# Package
tar -czf flask-app.tar.gz \
  requirements.txt \
  gunicorn.conf.py \
  app.py \
  templates/ \
  static/
```

## Distribution Packages

### Creating DEB Package (Debian/Ubuntu)

```bash
# Directory structure
mkdir -p miwidothttp-1.0.0/DEBIAN
mkdir -p miwidothttp-1.0.0/usr/local/bin
mkdir -p miwidothttp-1.0.0/etc/miwidothttp
mkdir -p miwidothttp-1.0.0/usr/lib/systemd/system

# Control file
cat > miwidothttp-1.0.0/DEBIAN/control << EOF
Package: miwidothttp
Version: 1.0.0
Section: web
Priority: optional
Architecture: amd64
Depends: libc6 (>= 2.31), libssl3 (>= 3.0.0)
Maintainer: Your Name <you@example.com>
Description: High-performance HTTP server with automatic SSL
 miwidothttp is a blazingly fast HTTP/HTTPS server written in Rust
 with automatic Cloudflare SSL integration and clustering support.
EOF

# Post-install script
cat > miwidothttp-1.0.0/DEBIAN/postinst << 'EOF'
#!/bin/bash
set -e

# Create user
if ! id -u miwidothttp >/dev/null 2>&1; then
    useradd -r -s /bin/false -d /var/lib/miwidothttp miwidothttp
fi

# Create directories
mkdir -p /var/log/miwidothttp
mkdir -p /var/cache/miwidothttp
mkdir -p /var/lib/miwidothttp

# Set permissions
chown -R miwidothttp:miwidothttp /var/log/miwidothttp
chown -R miwidothttp:miwidothttp /var/cache/miwidothttp
chown -R miwidothttp:miwidothttp /var/lib/miwidothttp

# Enable service
systemctl daemon-reload
systemctl enable miwidothttp

exit 0
EOF
chmod 755 miwidothttp-1.0.0/DEBIAN/postinst

# Copy files
cp target/release/miwidothttp miwidothttp-1.0.0/usr/local/bin/
cp config.toml miwidothttp-1.0.0/etc/miwidothttp/
cp systemd/miwidothttp.service miwidothttp-1.0.0/usr/lib/systemd/system/

# Build package
dpkg-deb --build miwidothttp-1.0.0
```

### Creating RPM Package (RHEL/CentOS/Fedora)

```bash
# Setup RPM build environment
mkdir -p ~/rpmbuild/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

# Spec file
cat > ~/rpmbuild/SPECS/miwidothttp.spec << 'EOF'
Name:           miwidothttp
Version:        1.0.0
Release:        1%{?dist}
Summary:        High-performance HTTP server with automatic SSL

License:        MIT
URL:            https://github.com/miwidothttp/miwidothttp
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  rust >= 1.82
Requires:       openssl-libs >= 3.0.0

%description
miwidothttp is a blazingly fast HTTP/HTTPS server written in Rust
with automatic Cloudflare SSL integration and clustering support.

%prep
%autosetup

%build
cargo build --release

%install
rm -rf $RPM_BUILD_ROOT
mkdir -p $RPM_BUILD_ROOT%{_bindir}
mkdir -p $RPM_BUILD_ROOT%{_sysconfdir}/miwidothttp
mkdir -p $RPM_BUILD_ROOT%{_unitdir}

install -m 755 target/release/miwidothttp $RPM_BUILD_ROOT%{_bindir}/
install -m 644 config.toml $RPM_BUILD_ROOT%{_sysconfdir}/miwidothttp/
install -m 644 systemd/miwidothttp.service $RPM_BUILD_ROOT%{_unitdir}/

%post
%systemd_post miwidothttp.service

%preun
%systemd_preun miwidothttp.service

%postun
%systemd_postun_with_restart miwidothttp.service

%files
%{_bindir}/miwidothttp
%config(noreplace) %{_sysconfdir}/miwidothttp/config.toml
%{_unitdir}/miwidothttp.service

%changelog
* Fri Aug 09 2025 Your Name <you@example.com> - 1.0.0-1
- Initial release
EOF

# Build RPM
rpmbuild -ba ~/rpmbuild/SPECS/miwidothttp.spec
```

### Creating Snap Package

```yaml
# snap/snapcraft.yaml
name: miwidothttp
base: core22
version: '1.0.0'
summary: High-performance HTTP server with automatic SSL
description: |
  miwidothttp is a blazingly fast HTTP/HTTPS server written in Rust
  with automatic Cloudflare SSL integration and clustering support.

grade: stable
confinement: classic

parts:
  miwidothttp:
    plugin: rust
    source: .
    build-packages:
      - cargo
      - rustc
      - pkg-config
      - libssl-dev
    stage-packages:
      - libssl3
      - ca-certificates

apps:
  miwidothttp:
    command: bin/miwidothttp
    daemon: simple
    restart-condition: always
    plugs:
      - network
      - network-bind
      - home
```

Build:
```bash
snapcraft
snap install miwidothttp_1.0.0_amd64.snap --classic
```

## Container Images

### Multi-Stage Docker Build

```dockerfile
# Dockerfile
# Build stage
FROM rust:1.82-alpine AS builder

RUN apk add --no-cache musl-dev openssl-dev

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --target x86_64-unknown-linux-musl

# Runtime stage
FROM alpine:3.19

RUN apk add --no-cache \
    ca-certificates \
    openssl \
    tini

RUN adduser -D -u 1000 miwidothttp

WORKDIR /app

COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/miwidothttp /usr/local/bin/
COPY config.toml /etc/miwidothttp/

RUN chown -R miwidothttp:miwidothttp /etc/miwidothttp

USER miwidothttp

EXPOSE 8080 8443

ENTRYPOINT ["/sbin/tini", "--"]
CMD ["miwidothttp", "--config", "/etc/miwidothttp/config.toml"]
```

### Building Multi-Architecture Images

```bash
# Setup buildx
docker buildx create --name multiarch --use
docker buildx inspect --bootstrap

# Build for multiple platforms
docker buildx build \
  --platform linux/amd64,linux/arm64,linux/arm/v7 \
  --tag miwidothttp/miwidothttp:latest \
  --push \
  .
```

### Minimal Container with Static Binary

```dockerfile
# Dockerfile.scratch
FROM rust:1.82 AS builder

# Build static binary
RUN rustup target add x86_64-unknown-linux-musl
RUN apt-get update && apt-get install -y musl-tools

WORKDIR /build
COPY . .

RUN RUSTFLAGS='-C target-feature=+crt-static' \
    cargo build --release --target x86_64-unknown-linux-musl

# Scratch image
FROM scratch

COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/miwidothttp /miwidothttp
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

EXPOSE 8080 8443

ENTRYPOINT ["/miwidothttp"]
```

## CI/CD Packaging

### GitHub Actions Release Pipeline

```yaml
# .github/workflows/release.yml
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            suffix: linux-amd64
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
            suffix: linux-arm64
          - os: macos-latest
            target: x86_64-apple-darwin
            suffix: darwin-amd64
          - os: macos-latest
            target: aarch64-apple-darwin
            suffix: darwin-arm64
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            suffix: windows-amd64.exe

    runs-on: ${{ matrix.os }}
    
    steps:
      - uses: actions/checkout@v3
      
      - name: Install Rust
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
          target: ${{ matrix.target }}
          
      - name: Build
        run: cargo build --release --target ${{ matrix.target }}
        
      - name: Package
        run: |
          mkdir -p dist
          cp target/${{ matrix.target }}/release/miwidothttp* dist/
          cp -r config-examples dist/
          cp README.md LICENSE dist/
          tar -czf miwidothttp-${{ matrix.suffix }}.tar.gz -C dist .
          
      - name: Upload artifact
        uses: actions/upload-artifact@v3
        with:
          name: miwidothttp-${{ matrix.suffix }}
          path: miwidothttp-${{ matrix.suffix }}.tar.gz

  release:
    needs: build
    runs-on: ubuntu-latest
    
    steps:
      - name: Download artifacts
        uses: actions/download-artifact@v3
        
      - name: Create Release
        uses: softprops/action-gh-release@v1
        with:
          files: |
            miwidothttp-*.tar.gz
            miwidothttp-*.exe
          draft: false
          prerelease: false
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

### GitLab CI Package Pipeline

```yaml
# .gitlab-ci.yml
stages:
  - build
  - package
  - release

variables:
  CARGO_HOME: ${CI_PROJECT_DIR}/.cargo

build:linux:
  stage: build
  image: rust:1.82
  script:
    - cargo build --release
    - mkdir -p artifacts
    - cp target/release/miwidothttp artifacts/
  artifacts:
    paths:
      - artifacts/
    expire_in: 1 hour

package:deb:
  stage: package
  image: debian:12
  dependencies:
    - build:linux
  script:
    - apt-get update && apt-get install -y dpkg-dev
    - mkdir -p package/DEBIAN
    - mkdir -p package/usr/local/bin
    - cp artifacts/miwidothttp package/usr/local/bin/
    - |
      cat > package/DEBIAN/control << EOF
      Package: miwidothttp
      Version: ${CI_COMMIT_TAG}
      Architecture: amd64
      Maintainer: GitLab CI
      Description: High-performance HTTP server
      EOF
    - dpkg-deb --build package miwidothttp_${CI_COMMIT_TAG}_amd64.deb
  artifacts:
    paths:
      - "*.deb"
    expire_in: 1 week

package:docker:
  stage: package
  image: docker:latest
  services:
    - docker:dind
  script:
    - docker build -t ${CI_REGISTRY_IMAGE}:${CI_COMMIT_TAG} .
    - docker push ${CI_REGISTRY_IMAGE}:${CI_COMMIT_TAG}
    - docker tag ${CI_REGISTRY_IMAGE}:${CI_COMMIT_TAG} ${CI_REGISTRY_IMAGE}:latest
    - docker push ${CI_REGISTRY_IMAGE}:latest

release:
  stage: release
  image: alpine:latest
  dependencies:
    - package:deb
  script:
    - apk add --no-cache curl
    - |
      curl --request POST \
        --header "PRIVATE-TOKEN: ${CI_JOB_TOKEN}" \
        --form "file=@miwidothttp_${CI_COMMIT_TAG}_amd64.deb" \
        "${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/packages/generic/miwidothttp/${CI_COMMIT_TAG}/miwidothttp_${CI_COMMIT_TAG}_amd64.deb"
  only:
    - tags
```

## Best Practices

### Version Management
- Use semantic versioning (MAJOR.MINOR.PATCH)
- Tag releases in git
- Include version in binary: `miwidothttp --version`
- Maintain changelog

### Security
- Sign packages with GPG
- Use checksums (SHA256)
- Scan for vulnerabilities
- Minimize container attack surface

### Optimization
- Strip debug symbols for production
- Use LTO (Link Time Optimization)
- Compress assets (gzip, brotli)
- Minimize container layers

### Testing
- Test packages in clean environments
- Verify upgrades and downgrades
- Check all dependencies
- Test on target platforms