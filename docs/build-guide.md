# Build Guide

## Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| Rust | 1.75+ | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Node.js | 18+ | `brew install node` |
| Go | 1.21+ | `brew install go` |
| Xcode CLT | latest | `xcode-select --install` |

## Development

```bash
npm install
npm run tauri dev
```

This starts the Vite dev server and launches the Tauri app with hot reload.

## Production Build

### Quick Build (current architecture)

```bash
./scripts/build.sh
```

This will:
1. Compile the Go sidecar (`feishu-gateway`) for your architecture
2. Build the Vue frontend (`dist/`)
3. Build the Tauri app with bundled sidecar

### Output

```
src-tauri/target/release/bundle/
├── macos/
│   └── CC Remote Seamless.app    # macOS app bundle
└── dmg/
    └── CC Remote Seamless_0.1.0_aarch64.dmg  # Installer
```

### Universal Binary (arm64 + x86_64)

```bash
# Install both targets
rustup target add x86_64-apple-darwin aarch64-apple-darwin

# Build sidecar for both archs
cd sidecar/feishu-gateway
CGO_ENABLED=0 GOARCH=amd64 go build -o feishu-gateway-x86_64-apple-darwin .
CGO_ENABLED=0 GOARCH=arm64 go build -o feishu-gateway-aarch64-apple-darwin .

# Build universal binary
cd ../..
npm run tauri build -- --target universal-apple-darwin
```

## Sidecar Details

The `feishu-gateway` Go binary communicates with the main app via stdio (JSON Lines protocol). Tauri requires sidecars to be named with the target triple suffix:

```
feishu-gateway-aarch64-apple-darwin   # Apple Silicon
feishu-gateway-x86_64-apple-darwin    # Intel Mac
```

The config in `tauri.conf.json`:
```json
"bundle": {
  "externalBin": ["../sidecar/feishu-gateway/feishu-gateway"]
}
```

Tauri automatically appends the target triple when bundling.

## Code Signing

### Without signing (development)

The `.app` will trigger macOS Gatekeeper. Users must right-click → Open to bypass.

### Ad-hoc signing (local distribution)

```bash
codesign --force --deep -s - "src-tauri/target/release/bundle/macos/CC Remote Seamless.app"
```

### Developer ID signing (public distribution)

Requires Apple Developer Program ($99/year):

```bash
codesign --force --deep -s "Developer ID Application: Your Name (TEAM_ID)" \
  "src-tauri/target/release/bundle/macos/CC Remote Seamless.app"
```

## Distribution Options

| Method | Audience | Notes |
|--------|----------|-------|
| Direct `.dmg` | Team/friends | Share via cloud storage |
| GitHub Release | Open source | CI builds via GitHub Actions |
| Homebrew Cask | macOS users | `brew install --cask cc-remote-seamless` |

## Hooks Setup (for users)

After installing the app, users need to register Claude Code hooks:

```bash
./scripts/install-hooks.sh
```

This configures Claude Code to send lifecycle events to the local hook server (port 23399).
