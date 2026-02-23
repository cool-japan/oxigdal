# Deployment Guide - OxiGDAL COG Viewer

Comprehensive guide for deploying the COG viewer to various platforms.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Build Process](#build-process)
- [Platform-Specific Guides](#platform-specific-guides)
  - [GitHub Pages](#github-pages)
  - [Netlify](#netlify)
  - [Vercel](#vercel)
  - [AWS S3 + CloudFront](#aws-s3--cloudfront)
  - [Docker](#docker)
  - [Self-Hosted Nginx](#self-hosted-nginx)
- [CORS Configuration](#cors-configuration)
- [Performance Optimization](#performance-optimization)
- [Security Considerations](#security-considerations)
- [Monitoring](#monitoring)

## Prerequisites

Before deploying, ensure:

1. WASM package is built:
   ```bash
   cd ../../crates/oxigdal-wasm
   wasm-pack build --target web --release --out-dir ../../demo/pkg
   ```

2. All files are present:
   ```bash
   cd ../../demo/cog-viewer
   ./verify.sh
   ```

3. Test locally:
   ```bash
   ./run.sh
   ```

## Build Process

### Production Build

```bash
# Navigate to oxigdal-wasm
cd ../../crates/oxigdal-wasm

# Build optimized WASM
wasm-pack build --target web --release --out-dir ../../demo/pkg

# Optional: Further optimize WASM
wasm-opt -Oz -o ../../demo/pkg/oxigdal_wasm_bg.wasm ../../demo/pkg/oxigdal_wasm_bg.wasm
```

### Build Optimization Options

```bash
# Maximum optimization (slower build, smaller size)
wasm-pack build --target web --release -- --features opt-level-z

# With debug info for profiling
wasm-pack build --target web --profiling --out-dir ../../demo/pkg
```

## Platform-Specific Guides

### GitHub Pages

#### Method 1: Deploy from Branch

1. Build WASM package
2. Commit to repository:
   ```bash
   git add demo/cog-viewer demo/pkg
   git commit -m "Add COG viewer demo"
   git push
   ```

3. Configure GitHub Pages:
   - Settings → Pages
   - Source: Deploy from branch
   - Branch: `main`, folder: `/demo/cog-viewer`
   - Save

4. Access at: `https://YOUR-USERNAME.github.io/oxigdal/`

#### Method 2: GitHub Actions

Create `.github/workflows/deploy-demo.yml`:

```yaml
name: Deploy COG Viewer Demo

on:
  push:
    branches: [main]
    paths:
      - 'demo/cog-viewer/**'
      - 'crates/oxigdal-wasm/**'

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - name: Setup Rust
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable

      - name: Install wasm-pack
        run: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

      - name: Build WASM
        run: |
          cd crates/oxigdal-wasm
          wasm-pack build --target web --release --out-dir ../../demo/pkg

      - name: Deploy to GitHub Pages
        uses: peaceiris/actions-gh-pages@v3
        with:
          github_token: ${{ secrets.GITHUB_TOKEN }}
          publish_dir: ./demo/cog-viewer
          publish_branch: gh-pages
```

### Netlify

#### Quick Deploy

1. Install Netlify CLI:
   ```bash
   npm install -g netlify-cli
   ```

2. Build WASM package

3. Create `netlify.toml` in `demo/cog-viewer/`:
   ```toml
   [build]
     publish = "."
     command = "echo 'No build needed - static site'"

   [[headers]]
     for = "/*"
     [headers.values]
       Access-Control-Allow-Origin = "*"
       Access-Control-Allow-Methods = "GET, HEAD, OPTIONS"
       Access-Control-Allow-Headers = "Range, Content-Type"

   [[headers]]
     for = "/*.wasm"
     [headers.values]
       Content-Type = "application/wasm"
       Cache-Control = "public, max-age=31536000, immutable"

   [[headers]]
     for = "/*.js"
     [headers.values]
       Cache-Control = "public, max-age=31536000, immutable"

   [[redirects]]
     from = "/*"
     to = "/index.html"
     status = 200
   ```

4. Deploy:
   ```bash
   cd demo/cog-viewer
   netlify deploy --prod
   ```

#### Continuous Deployment

1. Push to GitHub
2. Connect repository to Netlify
3. Configure build:
   - Base directory: `demo/cog-viewer`
   - Build command: (leave empty)
   - Publish directory: `.`
4. Deploy

### Vercel

#### CLI Deployment

1. Install Vercel CLI:
   ```bash
   npm install -g vercel
   ```

2. Create `vercel.json` in `demo/cog-viewer/`:
   ```json
   {
     "version": 2,
     "public": true,
     "headers": [
       {
         "source": "/(.*)",
         "headers": [
           { "key": "Access-Control-Allow-Origin", "value": "*" },
           { "key": "Access-Control-Allow-Methods", "value": "GET, HEAD, OPTIONS" },
           { "key": "Access-Control-Allow-Headers", "value": "Range, Content-Type" }
         ]
       },
       {
         "source": "/(.*)\\.wasm",
         "headers": [
           { "key": "Content-Type", "value": "application/wasm" },
           { "key": "Cache-Control", "value": "public, max-age=31536000, immutable" }
         ]
       },
       {
         "source": "/(.*)\\.js",
         "headers": [
           { "key": "Cache-Control", "value": "public, max-age=31536000, immutable" }
         ]
       }
     ],
     "rewrites": [
       { "source": "/(.*)", "destination": "/" }
     ]
   }
   ```

3. Deploy:
   ```bash
   cd demo/cog-viewer
   vercel --prod
   ```

#### Git Integration

1. Push to GitHub/GitLab/Bitbucket
2. Import repository in Vercel
3. Configure:
   - Framework Preset: Other
   - Root Directory: `demo/cog-viewer`
   - Build Command: (leave empty)
   - Output Directory: `.`
4. Deploy

### AWS S3 + CloudFront

#### S3 Configuration

1. Create S3 bucket:
   ```bash
   aws s3 mb s3://oxigdal-cog-viewer
   ```

2. Enable static website hosting:
   ```bash
   aws s3 website s3://oxigdal-cog-viewer --index-document index.html
   ```

3. Configure CORS:
   ```bash
   cat > cors.json << EOF
   {
     "CORSRules": [
       {
         "AllowedOrigins": ["*"],
         "AllowedMethods": ["GET", "HEAD"],
         "AllowedHeaders": ["*"],
         "ExposeHeaders": ["Content-Length", "Content-Range"],
         "MaxAgeSeconds": 3600
       }
     ]
   }
   EOF

   aws s3api put-bucket-cors --bucket oxigdal-cog-viewer --cors-configuration file://cors.json
   ```

4. Upload files:
   ```bash
   cd demo/cog-viewer
   aws s3 sync . s3://oxigdal-cog-viewer/ \
     --exclude ".git/*" \
     --exclude "*.sh" \
     --cache-control "public, max-age=31536000" \
     --metadata-directive REPLACE
   ```

5. Set proper MIME types:
   ```bash
   aws s3 cp s3://oxigdal-cog-viewer/ s3://oxigdal-cog-viewer/ \
     --recursive \
     --exclude "*" \
     --include "*.wasm" \
     --content-type "application/wasm" \
     --metadata-directive REPLACE
   ```

#### CloudFront Configuration

1. Create distribution:
   ```bash
   aws cloudfront create-distribution \
     --origin-domain-name oxigdal-cog-viewer.s3.amazonaws.com \
     --default-root-object index.html
   ```

2. Configure caching:
   - Cache policy: Managed-CachingOptimized
   - Origin request policy: CORS-S3Origin
   - Response headers policy: CORS-with-preflight-and-SecurityHeadersPolicy

### Docker

Create `Dockerfile` in `demo/cog-viewer/`:

```dockerfile
# Multi-stage build
FROM rust:1.75 AS builder

WORKDIR /app
COPY . .

# Install wasm-pack
RUN curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

# Build WASM
WORKDIR /app/crates/oxigdal-wasm
RUN wasm-pack build --target web --release --out-dir ../../demo/pkg

# Production stage
FROM nginx:alpine

# Copy nginx config
COPY <<EOF /etc/nginx/conf.d/default.conf
server {
    listen 80;
    server_name _;
    root /usr/share/nginx/html;
    index index.html;

    # CORS headers
    add_header Access-Control-Allow-Origin * always;
    add_header Access-Control-Allow-Methods "GET, HEAD, OPTIONS" always;
    add_header Access-Control-Allow-Headers "Range, Content-Type" always;

    # WASM MIME type
    location ~* \.wasm$ {
        types { application/wasm wasm; }
        add_header Cache-Control "public, max-age=31536000, immutable";
    }

    # JavaScript files
    location ~* \.js$ {
        add_header Cache-Control "public, max-age=31536000, immutable";
    }

    # HTML - no cache
    location ~* \.html$ {
        add_header Cache-Control "no-cache";
    }

    # SPA fallback
    location / {
        try_files \$uri \$uri/ /index.html;
    }
}
EOF

# Copy application files
COPY --from=builder /app/demo/cog-viewer /usr/share/nginx/html/
COPY --from=builder /app/demo/pkg /usr/share/nginx/html/pkg/

EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
```

Build and run:
```bash
cd ../../  # Project root
docker build -t oxigdal-cog-viewer -f demo/cog-viewer/Dockerfile .
docker run -p 8080:80 oxigdal-cog-viewer
```

### Self-Hosted Nginx

#### Nginx Configuration

Create `/etc/nginx/sites-available/oxigdal-cog-viewer`:

```nginx
server {
    listen 80;
    server_name demo.oxigdal.com;

    root /var/www/oxigdal-cog-viewer;
    index index.html;

    # Logging
    access_log /var/log/nginx/oxigdal-access.log;
    error_log /var/log/nginx/oxigdal-error.log;

    # CORS headers
    add_header Access-Control-Allow-Origin * always;
    add_header Access-Control-Allow-Methods "GET, HEAD, OPTIONS" always;
    add_header Access-Control-Allow-Headers "Range, Content-Type" always;
    add_header Access-Control-Expose-Headers "Content-Length, Content-Range" always;

    # Security headers
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-Content-Type-Options "nosniff" always;
    add_header X-XSS-Protection "1; mode=block" always;
    add_header Referrer-Policy "no-referrer-when-downgrade" always;

    # WASM files
    location ~* \.wasm$ {
        types { application/wasm wasm; }
        add_header Cache-Control "public, max-age=31536000, immutable";
        expires 1y;
    }

    # JavaScript files
    location ~* \.js$ {
        add_header Cache-Control "public, max-age=31536000, immutable";
        expires 1y;
    }

    # CSS files
    location ~* \.css$ {
        add_header Cache-Control "public, max-age=31536000, immutable";
        expires 1y;
    }

    # HTML files - no cache
    location ~* \.html$ {
        add_header Cache-Control "no-cache, no-store, must-revalidate";
        expires 0;
    }

    # Main location
    location / {
        try_files $uri $uri/ /index.html;
    }

    # Gzip compression
    gzip on;
    gzip_vary on;
    gzip_types text/plain text/css application/json application/javascript text/xml application/xml application/xml+rss text/javascript application/wasm;
}
```

Enable and restart:
```bash
sudo ln -s /etc/nginx/sites-available/oxigdal-cog-viewer /etc/nginx/sites-enabled/
sudo nginx -t
sudo systemctl restart nginx
```

Deploy files:
```bash
rsync -avz --delete demo/cog-viewer/ user@server:/var/www/oxigdal-cog-viewer/
```

## CORS Configuration

### Required Headers

```
Access-Control-Allow-Origin: *
Access-Control-Allow-Methods: GET, HEAD, OPTIONS
Access-Control-Allow-Headers: Range, Content-Type
Access-Control-Expose-Headers: Content-Length, Content-Range
```

### Testing CORS

```bash
curl -I -X OPTIONS \
  -H "Origin: https://your-domain.com" \
  -H "Access-Control-Request-Method: GET" \
  -H "Access-Control-Request-Headers: Range" \
  https://your-demo-url.com/
```

## Performance Optimization

### WASM Optimization

```bash
# Install wasm-opt
cargo install wasm-opt

# Optimize WASM binary
wasm-opt -Oz -o demo/pkg/oxigdal_wasm_bg.wasm demo/pkg/oxigdal_wasm_bg.wasm
```

### Compression

Enable Brotli and Gzip:
```bash
# Pre-compress files
find demo/cog-viewer -type f \( -name "*.js" -o -name "*.wasm" -o -name "*.css" \) -exec gzip -k {} \;
find demo/cog-viewer -type f \( -name "*.js" -o -name "*.wasm" -o -name "*.css" \) -exec brotli {} \;
```

### CDN Integration

Use CloudFlare, Fastly, or AWS CloudFront to:
- Cache static assets globally
- Enable HTTP/2 and HTTP/3
- Reduce latency with edge caching

## Security Considerations

### Content Security Policy

Add to HTML `<head>`:
```html
<meta http-equiv="Content-Security-Policy" content="
  default-src 'self';
  script-src 'self' 'wasm-unsafe-eval' https://unpkg.com;
  style-src 'self' 'unsafe-inline' https://unpkg.com;
  img-src 'self' data: https:;
  connect-src 'self' https:;
  font-src 'self' data:;
  worker-src 'self' blob:;
">
```

### HTTPS Only

Always use HTTPS in production:
```bash
# Let's Encrypt with certbot
sudo certbot --nginx -d demo.oxigdal.com
```

### Rate Limiting

Configure rate limiting in Nginx:
```nginx
limit_req_zone $binary_remote_addr zone=demo:10m rate=10r/s;

location / {
    limit_req zone=demo burst=20 nodelay;
    # ... rest of config
}
```

## Monitoring

### Error Tracking

Add to `main.js`:
```javascript
window.addEventListener('error', (event) => {
    // Send to error tracking service
    console.error('Global error:', event.error);
});
```

### Performance Monitoring

```javascript
// Track page load time
window.addEventListener('load', () => {
    const perfData = performance.timing;
    const pageLoadTime = perfData.loadEventEnd - perfData.navigationStart;
    console.log('Page load time:', pageLoadTime);
});
```

### Analytics

Add Google Analytics or Plausible:
```html
<!-- Plausible Analytics -->
<script defer data-domain="demo.oxigdal.com" src="https://plausible.io/js/script.js"></script>
```

## Verification

After deployment, verify:

1. **WASM loads**: Check browser console for errors
2. **CORS works**: Test loading example COG
3. **Caching**: Check response headers for `Cache-Control`
4. **Compression**: Verify Gzip/Brotli in network tab
5. **HTTPS**: Ensure SSL certificate is valid
6. **Performance**: Run Lighthouse audit

```bash
# Lighthouse audit
npx lighthouse https://your-demo-url.com --view
```

---

**Deployment Complete!** Your COG viewer is now live and ready to showcase OxiGDAL's browser capabilities.
