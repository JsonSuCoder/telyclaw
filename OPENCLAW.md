# OpenClaw Integration for Telegram-TT

This project integrates OpenClaw AI assistant capabilities into the Telegram web client.

## Setup

### 1. Install Dependencies
```bash
npm install
```

### 2. Build OpenClaw Runtime
```bash
# For development (starts runtime host)
npm run dev:openclaw

# For production build (Mac ARM64)
npm run build:openclaw

# For other platforms
npm run openclaw:runtime:mac-x64    # Mac Intel
npm run openclaw:runtime:linux-x64  # Linux x64
npm run openclaw:runtime:win-x64    # Windows x64
```

### 3. Development
```bash
# Web development with OpenClaw integration
npm run dev:openclaw

# Web development (without OpenClaw)
npm run dev

# Tauri development with OpenClaw runtime host
npm run tauri:dev

# Tauri development with full OpenClaw build
npm run tauri:dev:openclaw

# Tauri production build (includes OpenClaw)
npm run tauri:build
```

## OpenClaw Configuration

OpenClaw configuration is defined in `package.json`:

```json
{
  "openclaw": {
    "version": "v2026.3.2",
    "repo": "https://github.com/openclaw/openclaw.git",
    "plugins": [
      {
        "id": "telegram-openclaw-plugin",
        "npm": "@telegram/openclaw-plugin",
        "version": "1.0.0",
        "optional": true
      }
    ]
  }
}
```

## Available Scripts

### Development Scripts
- **`npm run dev:openclaw`** - Web development with OpenClaw integration
- **`npm run tauri:dev`** - Tauri development with OpenClaw runtime host
- **`npm run tauri:dev:openclaw`** - Tauri development with full OpenClaw build

### Build Scripts
- **`npm run build:openclaw`** - Build OpenClaw runtime for current platform
- **`npm run tauri:build`** - Build Tauri application with OpenClaw integration

### Platform-Specific Runtime Builds
- **`npm run openclaw:runtime:mac-arm64`** - Build for Mac ARM64
- **`npm run openclaw:runtime:mac-x64`** - Build for Mac Intel
- **`npm run openclaw:runtime:linux-x64`** - Build for Linux x64
- **`npm run openclaw:runtime:win-x64`** - Build for Windows x64

### Individual Build Steps
- **`npm run openclaw:ensure`** - Clone OpenClaw repository
- **`npm run openclaw:patch`** - Apply local patches
- **`npm run openclaw:bundle`** - Bundle the gateway
- **`npm run openclaw:plugins`** - Install configured plugins
- **`npm run openclaw:extensions:local`** - Sync local extensions
- **`npm run openclaw:precompile`** - Precompile extensions
- **`npm run openclaw:prune`** - Remove unnecessary files

## Directory Structure

```
telegram-tt/
├── src/openclaw/              # OpenClaw integration
│   ├── scripts/              # Build scripts
│   ├── patches/              # OpenClaw patches
│   ├── runtime/              # Platform-specific runtime (generated)
│   ├── gateway/              # Bundled gateway (generated)
│   └── openclaw-extensions/  # Local extensions
└── package.json              # OpenClaw configuration
```

## Customization

- **Add patches**: Place `.patch` files in `./src/openclaw/patches/` directory
- **Local extensions**: Add to `./src/openclaw/openclaw-extensions/`
- **Plugins**: Configure in `package.json` under `openclaw.plugins`

## Troubleshooting

- Ensure Node.js version matches requirements (^22.6 || ^24)
- Run `npm run openclaw:ensure` if OpenClaw source is missing
- Check `../openclaw` directory for build artifacts
- Review build logs for specific error messages

### Common Issues

**OpenClaw directory not found:**
```bash
npm run openclaw:ensure
```

**Build failures:**
- Check Node.js version compatibility
- Ensure all dependencies are installed: `npm install`
- Try cleaning and rebuilding: `rm -rf ../openclaw && npm run build:openclaw`

**Tauri development issues:**
- Use `npm run tauri:dev:openclaw` for full rebuild
- Check that OpenClaw runtime host is running
- Verify Tauri prerequisites are installed

**Plugin installation failures:**
- Check network connectivity
- Verify plugin versions in `package.json`
- Optional plugins will show warnings but won't fail the build

**Dependency conflicts (ERESOLVE errors):**
- Scripts automatically retry with `--legacy-peer-deps` and `--force` flags
- If issues persist, manually clean and rebuild:
  ```bash
  rm -rf ../openclaw/node_modules
  rm -rf ../openclaw/package-lock.json
  npm run openclaw:ensure
  npm run build:openclaw
  ```