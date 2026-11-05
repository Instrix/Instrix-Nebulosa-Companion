# Nebulosa Companion

[![CI](https://github.com/Instrix/Instrix-Nebulosa-Companion/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/Instrix/Instrix-Nebulosa-Companion/actions/workflows/ci.yml)
[![Release](https://github.com/Instrix/Instrix-Nebulosa-Companion/actions/workflows/release.yml/badge.svg)](https://github.com/Instrix/Instrix-Nebulosa-Companion/actions/workflows/release.yml)
[![Download](https://img.shields.io/badge/download-latest_release-2ea44f?logo=github)](https://github.com/Instrix/Instrix-Nebulosa-Companion/releases/latest)
![Version](https://img.shields.io/badge/version-0.1.1-blue)
![Platforms](https://img.shields.io/badge/platforms-Windows_%7C_macOS_%7C_Linux_%7C_browser-informational)
![License](https://img.shields.io/badge/license-proprietary-lightgrey)

![Rust](https://img.shields.io/badge/Rust-backend_%2B_WASM-B7410E?logo=rust)
![Svelte](https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![Electron](https://img.shields.io/badge/Electron-Linux-47848F?logo=electron&logoColor=white)

Companion app for the INSTRIX Nebulosa power profiler
([Instrix-Nebulosa-Firmware](https://github.com/Instrix/Instrix-Nebulosa-Firmware)). It talks NBP (the
Nebulosa Bulk Protocol) over USB or Ethernet, and runs as a desktop app
(Windows, Linux, macOS) or in a browser.

## Download

**[⬇ Latest release](https://github.com/Instrix/Instrix-Nebulosa-Companion/releases/latest)**: installers for every platform, with
`SHA256SUMS.txt` · [all releases](https://github.com/Instrix/Instrix-Nebulosa-Companion/releases) · [manual](https://github.com/Instrix/Instrix-Nebulosa-Docs/blob/main/dist/UM-0003_RevA_nebulosa-companion.pdf) · [product brief](https://github.com/Instrix/Instrix-Nebulosa-Docs/blob/main/dist/PB-0002_RevA_nebulosa-companion.pdf)

| System | Get | Notes |
|:--|:--|:--|
| Windows 10 / 11 (x64, arm64) | `…_x64-setup.exe` (or `.msi`, portable `.zip`) | Unsigned: SmartScreen › **More info** › **Run anyway**. USB: WinUSB with [Zadig](https://zadig.akeo.ie) until the firmware has MS OS 2.0 descriptors |
| macOS (Apple silicon, Intel) | `…_universal.dmg` | Ad-hoc signed: first launch with right-click › **Open** |
| Linux (x86_64, arm64) | `.deb` (Debian, Ubuntu), `.rpm` (Fedora), `.AppImage` | `.deb` / `.rpm` install the udev rule; replug the board after installing |
| Browser | Chrome or Edge | WebUSB: USB only |

The repository is private: the links and the CI badges need a GitHub account with access.

## Layout

```
crates/nbp-core/   Rust, no I/O: NBP framing, messages, format-5 stream decoder, display
                   reduction. Built natively for the desktop app and to WASM for the UI.
src-tauri/         the desktop backend: USB (nusb), TCP, UDP discovery, serial, files.
                   Commands in api.rs; two shells over them: Tauri (shell_tauri.rs,
                   Windows, macOS) and a WebSocket server (server.rs, bin/nebulosa-backend)
electron/          the Linux shell: a Chromium window on nebulosa-backend, and its packaging
ui/                Svelte 5 + uPlot. One Device API over two transports:
                   lib/native.ts (the backend, through lib/ipc.ts) and lib/webusb.ts (browser)
udev/              Linux permissions for the USB devices
```

```
UI (Svelte) ── Device ──┬── native.ts ── ipc.ts ─┬─ Tauri IPC ───────┬─ src-tauri: nusb / tokio TCP ──┐
                        │                        └─ WebSocket (Linux) ┘                               │
                        └── webusb.ts ── WebUSB ────────────────────────────────────────────────┐     │
                                                                                nbp-core (WASM) ┘     └ nbp-core (native)
```

Why two desktop shells: on Linux, Tauri draws with WebKitGTK, which renders the
plots far slower than Chromium. The Linux app therefore runs the same backend on
its own (`nebulosa-backend`, no WebKitGTK linked), serving the UI and a WebSocket
on 127.0.0.1 that wants the token it prints; Electron opens it in a window and
stops it on quit. Windows (WebView2, Chromium) and macOS (WebKit) stay on Tauri.

The receive side (frame parsing, DATA block checks, stream decoding, min/max
reduction to about 4000 points/s) is the same Rust code in both builds. The UI
parses responses and encodes payloads through the WASM build, so payload layouts
are defined only in `crates/nbp-core/src/msg.rs`. Keep that file in step with
`common/nbp/nbp.h` in the firmware.

## Setup

```sh
# Rust, the WASM target, wasm-pack
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --locked

# Linux: the backend's libraries (and Tauri's, only to run the Tauri shell here)
sudo apt install libudev-dev build-essential pkg-config
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libayatana-appindicator3-dev librsvg2-dev
# Linux: USB access without root
sudo cp udev/99-nebulosa.rules /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger

npm install
(cd electron && npm install)   # Linux shell
```

## Run

```sh
npm run linux      # Linux desktop app: UI, backend, Electron window
npm run linux:dist # Linux packages (target/electron)
npm run dev        # Tauri desktop app (Windows, macOS; on Linux it runs, slowly)
npm run web        # browser build at http://localhost:5173 (Chrome / Edge: WebUSB)
npm run build      # Tauri installers (target/release/bundle)
npm run build:ui   # static browser build in dist/
npm test           # nbp-core unit tests
```

Without hardware: run the debugger's simulator
(`../Instrix-Nebulosa-Debugger/build/nebulosa-sim`) and connect to `127.0.0.1`.

## Packages and releases

GitHub Actions builds every package (`.github/workflows/packages.yml`):

| Platform | Packages |
|:--|:--|
| Linux (x86_64, arm64) | `.deb`, `.rpm` (both install the udev rule), `.AppImage`: Electron |
| Windows (x64, arm64) | NSIS installer `-setup.exe`, `.msi` (x64 only), portable `.exe` (zip, needs WebView2) |
| macOS (universal: Apple silicon + Intel) | `.dmg`, `.app` (zip) |

Windows arm64 is cross-compiled on the x64 runner; Linux arm64 builds on GitHub's arm64
runner. The arm64 jobs may fail without blocking a release.

- **CI** (`ci.yml`): tests on every push and pull request; on `main` and manual runs also
  the packages, as build artifacts.
- **Release** (`release.yml`): push a tag matching the version in `src-tauri/tauri.conf.json`
  (`git tag v0.2.0 && git push origin v0.2.0`); it builds everything and opens a **draft**
  GitHub Release with the packages and `SHA256SUMS.txt`. Review it, then publish.
  The version lives in `src-tauri/tauri.conf.json`, `Cargo.toml`, `package.json`,
  `electron/package.json` and the version badge at the top of this file (a private
  repository can't have it read from the releases).
- Builds are unsigned (macOS gets an ad-hoc signature), so Windows SmartScreen and macOS
  Gatekeeper warn on first launch. Signing hooks are noted at the end of `packages.yml`.

Locally, one platform at a time: `npm run linux:dist` (Linux, in `target/electron/`);
`npx tauri build --bundles nsis,msi` (Windows), `app,dmg` (macOS), in `target/release/bundle/`.

## Platform notes

- **Browser:** WebUSB works only in Chromium-based browsers. Browsers have no raw TCP/UDP, so
  the browser build is USB only. Ethernet would need a WebSocket endpoint in the firmware.
- **Windows:** the NBP interface must bind WinUSB, both for the desktop app (nusb) and for
  WebUSB. That needs MS OS 2.0 descriptors in the firmware (to do), or Zadig until then.
- **VID/PID:** `1A86` is WCH's vendor ID. A product release needs its own.
- **Units:** the plot shows raw 14-bit codes per range. Calibration to amps and volts is to do.
- **Stream format:** only format 5 (FPGA ID `0x4E42000B` and later) is decoded.
