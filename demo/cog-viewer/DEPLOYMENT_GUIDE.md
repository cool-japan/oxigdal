# OxiGDAL COG Viewer Deployment Guide

Complete guide for deploying the OxiGDAL COG Viewer to various platforms.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Building for Production](#building-for-production)
- [GitHub Pages Deployment](#github-pages-deployment)
- [Netlify Deployment](#netlify-deployment)
- [Vercel Deployment](#vercel-deployment)
- [AWS S3 + CloudFront](#aws-s3--cloudfront)
- [Custom Server Deployment](#custom-server-deployment)
- [Troubleshooting](#troubleshooting)

## Prerequisites

### Required Tools

1. **Rust** (1.85 or later)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup target add wasm32-unknown-unknown
   ```

2. **wasm-pack**
   ```bash
   cargo install wasm-pack
   ```

3. **wasm-opt** (optional, for optimization)
   ```bash
   # macOS
   brew install binaryen

   # Ubuntu/Debian
   apt-get install binaryen

   # From source
   wget https://github.com/WebAssembly/binaryen/releases/latest
   ```

## Building for Production

### Step 1: Build WASM Package

From the project root:

```bash
cd crates/oxigdal-wasm
wasm-pack build --target web --release --out-dir ../../demo/pkg
```

This creates:
- `oxigdal_wasm.js` - JavaScript bindings
- `oxigdal_wasm_bg.wasm` - WebAssembly binary
- `oxigdal_wasm.d.ts` - TypeScript definitions

### Step 2: Optimize WASM (Optional)

```bash
cd ../../demo/pkg
wasm-opt -Oz -o oxigdal_wasm_bg_opt.wasm oxigdal_wasm_bg.wasm
mv oxigdal_wasm_bg_opt.wasm oxigdal_wasm_bg.wasm
```

Optimization flags:
- `-Oz` - Optimize for size (recommended for web)
- `-O3` - Optimize for speed
- `-O4` - Maximum optimization

### Step 3: Verify Build

```bash
cd ../cog-viewer
./verify.sh
```

## GitHub Pages Deployment

### Automatic Deployment (GitHub Actions)

The repository includes a pre-configured workflow at `.github/workflows/deploy.yml`.

#### Setup

1. Enable GitHub Pages in repository settings:
   - Settings → Pages
   - Source: GitHub Actions

2. Push to main branch:
   ```bash
   git add .
   git commit -m "Deploy COG viewer"
   git push origin main
   ```

3. View deployment:
   - Actions tab will show build progress
   - Site will be available at `https://USERNAME.github.io/oxigdal/`

#### Manual Deployment

```bash
# Build
npm run build

# Create gh-pages branch
git checkout -b gh-pages
git add -f demo/pkg demo/cog-viewer
git commit -m "Deploy to GitHub Pages"
git push origin gh-pages

# Configure GitHub Pages to use gh-pages branch
```

### Custom Domain

1. Add `CNAME` file:
   ```bash
   echo "cog-viewer.example.com" > demo/cog-viewer/CNAME
   ```

2. Configure DNS:
   ```
   CNAME record: cog-viewer → USERNAME.github.io
   ```

## Netlify Deployment

### Option 1: Netlify CLI

```bash
# Install Netlify CLI
npm install -g netlify-cli

# Login
netlify login

# Build and deploy
npm run build
netlify deploy --prod --dir=demo/cog-viewer
```

### Option 2: Git Integration

1. Connect repository to Netlify

2. Configure build settings:
   - **Build command:** `cd crates/oxigdal-wasm && wasm-pack build --target web --release --out-dir ../../demo/pkg`
   - **Publish directory:** `demo/cog-viewer`
   - **Environment variables:**
     ```
     RUST_VERSION=1.85
     ```

3. Deploy:
   - Push to main branch
   - Netlify auto-deploys

### Option 3: Drag and Drop

```bash
# Build locally
npm run build

# Zip the directory
cd demo
zip -r cog-viewer.zip cog-viewer pkg

# Upload to Netlify dashboard
```

### Netlify Configuration

The included `netlify.toml`:

```toml
[build]
  command = "cd ../../crates/oxigdal-wasm && wasm-pack build --target web --release --out-dir ../../demo/pkg"
  publish = "."

[build.environment]
  RUST_VERSION = "1.85"

[[redirects]]
  from = "/*"
  to = "/index.html"
  status = 200

[[headers]]
  for = "/*.wasm"
  [headers.values]
    Content-Type = "application/wasm"
    Cache-Control = "public, max-age=31536000, immutable"

[[headers]]
  for = "/*.js"
  [headers.values]
    Cache-Control = "public, max-age=31536000, immutable"
```

## Vercel Deployment

### Option 1: Vercel CLI

```bash
# Install Vercel CLI
npm install -g vercel

# Login
vercel login

# Deploy
npm run build
vercel --prod
```

### Option 2: Git Integration

1. Import repository in Vercel dashboard

2. Configure build settings:
   - **Framework Preset:** Other
   - **Build Command:** `cd crates/oxigdal-wasm && wasm-pack build --target web --release --out-dir ../../demo/pkg`
   - **Output Directory:** `demo/cog-viewer`

3. Add environment variables:
   ```
   RUST_VERSION=1.85
   ```

### Vercel Configuration

The included `vercel.json`:

```json
{
  "version": 2,
  "builds": [
    {
      "src": "crates/oxigdal-wasm/Cargo.toml",
      "use": "@vercel/rust",
      "config": {
        "target": "wasm32-unknown-unknown"
      }
    }
  ],
  "routes": [
    {
      "src": "/(.*)",
      "dest": "/demo/cog-viewer/$1"
    }
  ],
  "headers": [
    {
      "source": "/(.*).wasm",
      "headers": [
        {
          "key": "Content-Type",
          "value": "application/wasm"
        },
        {
          "key": "Cache-Control",
          "value": "public, max-age=31536000, immutable"
        }
      ]
    }
  ]
}
```

## AWS S3 + CloudFront

### Step 1: Build and Prepare

```bash
npm run build
cd demo
```

### Step 2: Create S3 Bucket

```bash
aws s3 mb s3://oxigdal-cog-viewer
aws s3 website s3://oxigdal-cog-viewer \
  --index-document index.html \
  --error-document 404.html
```

### Step 3: Upload Files

```bash
# Upload with correct MIME types
aws s3 sync cog-viewer s3://oxigdal-cog-viewer \
  --exclude "*.wasm" \
  --cache-control "public, max-age=31536000"

aws s3 sync pkg s3://oxigdal-cog-viewer/pkg \
  --exclude "*" \
  --include "*.wasm" \
  --content-type "application/wasm" \
  --cache-control "public, max-age=31536000"

aws s3 sync pkg s3://oxigdal-cog-viewer/pkg \
  --exclude "*.wasm"
```

### Step 4: Configure Public Access

```bash
aws s3api put-bucket-policy --bucket oxigdal-cog-viewer --policy '{
  "Version": "2012-10-17",
  "Statement": [{
    "Sid": "PublicReadGetObject",
    "Effect": "Allow",
    "Principal": "*",
    "Action": "s3:GetObject",
    "Resource": "arn:aws:s3:::oxigdal-cog-viewer/*"
  }]
}'
```

### Step 5: Create CloudFront Distribution

```bash
aws cloudfront create-distribution --distribution-config '{
  "CallerReference": "oxigdal-cog-viewer-'$(date +%s)'",
  "Comment": "OxiGDAL COG Viewer",
  "Origins": {
    "Quantity": 1,
    "Items": [{
      "Id": "S3-oxigdal-cog-viewer",
      "DomainName": "oxigdal-cog-viewer.s3-website-us-east-1.amazonaws.com",
      "CustomOriginConfig": {
        "HTTPPort": 80,
        "OriginProtocolPolicy": "http-only"
      }
    }]
  },
  "DefaultCacheBehavior": {
    "TargetOriginId": "S3-oxigdal-cog-viewer",
    "ViewerProtocolPolicy": "redirect-to-https",
    "Compress": true,
    "MinTTL": 0,
    "ForwardedValues": {
      "QueryString": false,
      "Cookies": {"Forward": "none"}
    }
  },
  "Enabled": true
}'
```

## Custom Server Deployment

### Nginx Configuration

```nginx
server {
    listen 80;
    server_name cog-viewer.example.com;
    root /var/www/oxigdal-cog-viewer;
    index index.html;

    # WASM MIME type
    types {
        application/wasm wasm;
    }

    # Enable compression
    gzip on;
    gzip_types application/javascript application/wasm text/css;

    # CORS headers (if needed)
    add_header Access-Control-Allow-Origin *;
    add_header Cross-Origin-Embedder-Policy require-corp;
    add_header Cross-Origin-Opener-Policy same-origin;

    # Cache static assets
    location ~* \.(wasm|js|css)$ {
        expires 1y;
        add_header Cache-Control "public, immutable";
    }

    # SPA fallback
    location / {
        try_files $uri $uri/ /index.html;
    }
}
```

### Apache Configuration

```apache
<VirtualHost *:80>
    ServerName cog-viewer.example.com
    DocumentRoot /var/www/oxigdal-cog-viewer

    # WASM MIME type
    AddType application/wasm .wasm

    # Enable compression
    <IfModule mod_deflate.c>
        AddOutputFilterByType DEFLATE application/javascript
        AddOutputFilterByType DEFLATE application/wasm
        AddOutputFilterByType DEFLATE text/css
    </IfModule>

    # CORS headers
    Header set Access-Control-Allow-Origin "*"
    Header set Cross-Origin-Embedder-Policy "require-corp"
    Header set Cross-Origin-Opener-Policy "same-origin"

    # Cache static assets
    <FilesMatch "\.(wasm|js|css)$">
        Header set Cache-Control "public, max-age=31536000, immutable"
    </FilesMatch>

    # SPA fallback
    <Directory /var/www/oxigdal-cog-viewer>
        Options -Indexes +FollowSymLinks
        AllowOverride All
        Require all granted

        RewriteEngine On
        RewriteBase /
        RewriteRule ^index\.html$ - [L]
        RewriteCond %{REQUEST_FILENAME} !-f
        RewriteCond %{REQUEST_FILENAME} !-d
        RewriteRule . /index.html [L]
    </Directory>
</VirtualHost>
```

## Troubleshooting

### WASM Loading Errors

**Problem:** `TypeError: Failed to fetch` or CORS errors

**Solution:**
1. Ensure WASM MIME type is set:
   ```
   Content-Type: application/wasm
   ```

2. Enable CORS if loading from different origin:
   ```
   Access-Control-Allow-Origin: *
   ```

3. Check browser console for detailed error messages

### Module Not Found Errors

**Problem:** `Error: Cannot find module '../pkg/oxigdal_wasm.js'`

**Solution:**
```bash
# Rebuild WASM package
cd crates/oxigdal-wasm
wasm-pack build --target web --release --out-dir ../../demo/pkg

# Verify output
ls -la ../../demo/pkg
```

### Large WASM Size

**Problem:** WASM file is too large (>5MB)

**Solutions:**
1. Enable optimization:
   ```bash
   wasm-opt -Oz -o output.wasm input.wasm
   ```

2. Strip debug symbols:
   ```toml
   [profile.release]
   strip = true
   lto = true
   codegen-units = 1
   ```

3. Enable compression on server (gzip/brotli)

### Performance Issues

**Problem:** Slow loading or rendering

**Solutions:**
1. Enable CDN caching
2. Use HTTP/2 or HTTP/3
3. Implement progressive loading
4. Optimize tile size and caching strategy
5. Use Web Workers for tile processing

### CORS Issues with COG URLs

**Problem:** Cannot load COG from remote URL

**Solution:**
1. Ensure COG server supports CORS
2. Check HTTP range request support:
   ```bash
   curl -I -H "Range: bytes=0-1023" https://example.com/file.tif
   ```
3. Use CORS proxy for development:
   ```javascript
   const proxyUrl = 'https://cors-anywhere.herokuapp.com/';
   const cogUrl = proxyUrl + originalUrl;
   ```

## Performance Optimization

### Enable Compression

**Brotli compression** (best):
```nginx
brotli on;
brotli_types application/wasm application/javascript;
```

**Gzip compression** (fallback):
```nginx
gzip on;
gzip_types application/wasm application/javascript;
```

### Implement Service Worker

Create `sw.js`:
```javascript
self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open('oxigdal-v1').then((cache) => {
      return cache.addAll([
        '/index.html',
        '/main.js',
        '/style.css',
        '/pkg/oxigdal_wasm.js',
        '/pkg/oxigdal_wasm_bg.wasm',
      ]);
    })
  );
});

self.addEventListener('fetch', (event) => {
  event.respondWith(
    caches.match(event.request).then((response) => {
      return response || fetch(event.request);
    })
  );
});
```

### Enable HTTP/2 Server Push

```nginx
location = /index.html {
    http2_push /main.js;
    http2_push /style.css;
    http2_push /pkg/oxigdal_wasm.js;
}
```

## Monitoring and Analytics

### Add Google Analytics

```html
<script async src="https://www.googletagmanager.com/gtag/js?id=GA_MEASUREMENT_ID"></script>
<script>
  window.dataLayer = window.dataLayer || [];
  function gtag(){dataLayer.push(arguments);}
  gtag('js', new Date());
  gtag('config', 'GA_MEASUREMENT_ID');
</script>
```

### Add Performance Monitoring

```javascript
// Track WASM loading time
const wasmLoadStart = performance.now();
await init();
const wasmLoadTime = performance.now() - wasmLoadStart;
console.log(`WASM loaded in ${wasmLoadTime}ms`);
```

## Security Considerations

1. **Content Security Policy**
   ```html
   <meta http-equiv="Content-Security-Policy"
         content="default-src 'self'; script-src 'self' 'unsafe-eval';
                  connect-src 'self' https:; img-src 'self' data: https:;">
   ```

2. **HTTPS Only**
   - Always deploy with HTTPS enabled
   - Use HSTS header

3. **Subresource Integrity**
   ```html
   <script src="main.js" integrity="sha384-..." crossorigin="anonymous"></script>
   ```

## Cost Estimation

### GitHub Pages
- **Free** for public repositories
- 100GB bandwidth/month
- Custom domain support

### Netlify
- **Free tier:** 100GB bandwidth/month
- Pro: $19/month, 400GB bandwidth
- Auto SSL, CDN included

### Vercel
- **Free tier:** 100GB bandwidth/month
- Pro: $20/month
- Serverless functions included

### AWS S3 + CloudFront
- **S3:** $0.023/GB storage
- **CloudFront:** $0.085/GB transfer (first 10TB)
- **Estimate:** ~$10-30/month for moderate traffic

## Support

For issues and questions:
- GitHub Issues: https://github.com/cool-japan/oxigdal/issues
- Documentation: https://github.com/cool-japan/oxigdal/wiki
- Email: team@cooljapan.eu
