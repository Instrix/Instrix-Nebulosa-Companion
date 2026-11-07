# Nebulosa Companion

[![CI](https://github.com/Instrix/Instrix-Nebulosa-Companion/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/Instrix/Instrix-Nebulosa-Companion/actions/workflows/ci.yml)
[![Release](https://github.com/Instrix/Instrix-Nebulosa-Companion/actions/workflows/release.yml/badge.svg)](https://github.com/Instrix/Instrix-Nebulosa-Companion/actions/workflows/release.yml)
[![Download](https://img.shields.io/badge/download-latest_release-2ea44f?logo=github)](https://github.com/Instrix/Instrix-Nebulosa-Companion/releases/latest)
![Version](https://img.shields.io/badge/version-0.3.1-blue)
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

### Debug tab (build option)

The Nebulosa Debugger's functions in a **Debug** tab (Ctrl 4): the device's and
the host's stream counters with a throughput plot; the range switch (EN_TAP
outputs, comparators, manual / automatic, timing in FPGA clock cycles, forced
comparators); the RS1473 ADC link (lock, phases, change maps, eye, round trip,
RECAL, fixed / measured phases, clk_fast / echo engine, phase sweep); all 45
FPGA registers, read and write; the board's low-level controls (LEDs, USB3
mux, PROGRAM_B, screen, its UI driven from the keyboard); the SPI flash (read,
save, erase, write and verify); PING round trips; any raw NBP command; a log.

It is left out of the normal builds (and the releases). The debug variants:

```sh
npm run linux:debug       # Linux desktop app with the Debug tab
npm run linux:dist:debug  # Linux packages with it
npm run dev:debug         # Tauri, with it
npm run build:debug       # Tauri installers with it
npm run dev:ui:debug      # browser, with it (also: npm run build:ui:debug)
```

The tab has two parts. **Tools** is the above. **AFE bring-up** is the
debugger's bring-up tab (Instrix-Nebulosa `Design/afe1-x10-bringup.md`):

- the bench: a Siglent SPD3303C (`/dev/usbtmcN`) and a Korad KEL102 (its serial
  port, 115200), or `tcp:HOST[:PORT]` for SCPI over LAN, or `sim` for simulated
  ones; the desktop backend carries the lines (`src-tauri/src/instruments.rs`),
  the SCPI is in `ui/src/lib/bringup/instruments.ts`;
- **ALL OFF** (the load's input and CH1), also when the Nebulosa's link is lost
  and when the window closes; the load interlocked to 1.1 × FS of the tap that
  is on; taps stepped down one at a time;
- the **playbook**: tap identification with the DMM, or every tap / one tap
  (zero, then 10 / 50 / 85 % FS from the load or through the decade box), with
  operator dialogs, expected against read, pass / fail limits, retry / skip /
  back / jump;
- the **bench**: instrument controls, taps, the 0.5 µs bring-up overlap, measure
  against the load / V over the box / a typed reference, and the sequences:
  zero + noise, linearity, ladder ratio transfer, comparator trip points;
- **results**, saved as CSV.

Measurements read the raw ADC codes of one range through the core's probe
(`Pipeline::probe_arm` / `probe_take`). With the demo device and `sim`
instruments, the simulated loop current drives the demo's ADC, so the whole
playbook runs without hardware (every tap: 92 steps, 78 checks).

`NEBULOSA_DEBUG=1` before any build does the same; in the UI, `?mode=debug`
opens on the tab. The demo device (`?demo`) simulates the registers, the range
switch and an RS1473 link, so the tab can be tried without hardware. Code:
`ui/src/lib/DebugView.svelte`, `ui/src/lib/debug.ts`, `crates/nbp-core/src/debug.rs`.

## Remote control

**Settings → Remote control** serves the Companion on a port (default 8765) of this
computer, or also of the local network. Open the link it shows in a browser: the page
mirrors the app's window. It joins the device (or recording) the window has open, and
the instrument's settings (acquisition, trigger, logger) and streaming stay in step
both ways; the view, layout and theme are each browser's own. A demo device stays in
the UI that started it. The desktop app only (the backend owns the device); the link
carries a token, kept across restarts (Settings → New token revokes it).

The same port has a JSON API for scripts and test benches; the app's window carries
the calls out:

```sh
T=…   # the token, from Settings
curl -H "Authorization: Bearer $T" http://127.0.0.1:8765/api/v1/status
curl -H "Authorization: Bearer $T" -d '{"key":"usb:2:114"}' http://127.0.0.1:8765/api/v1/connect
curl -H "Authorization: Bearer $T" -X POST http://127.0.0.1:8765/api/v1/start
curl -H "Authorization: Bearer $T" "http://127.0.0.1:8765/api/v1/measure?seconds=1"
```

| Action | Arguments | Does |
|---|---|---|
| `status` | | connected, streaming, rate, mode, recording / logging, the settings |
| `devices` | | scans; the keys for `connect` |
| `connect` | `key`, or `host` [`port`] | opens a device (no key: the first found) |
| `demo` / `disconnect` | | the demo device / closes |
| `start` / `stop` | | streaming |
| `measure` | `seconds` (1) | mean, min, max, σ of current, voltage, power; charge, energy |
| `value` | | the newest sample |
| `settings` / `set` | `acquisition`, `trigger`, `logger`, `display`, `view` | as in a configuration file |
| `markers` / `marker` / `clear_markers` | `t0` [`t1`] [`name`] | markers and their measurements |
| `record` / `log` | `on` | raw recording / the session log to a file |
| `mode` | `scope` or `logger` | the view |
| `screenshot` | | the scope as PNG (base64) |

Answers are `{"ok": …}` or `{"error": "…"}`. On the local network anyone with the link
controls the instrument, reads and writes files and runs trigger scripts on this
computer, over plain http: turn it on for a network you trust only.

**Never expose it to the internet**: no port forwarding on the router, public reverse
proxy, tunnel (ngrok, Cloudflare Tunnel, …) or firewall rule opening it to the outside.
It is meant for this computer or a trusted local network only.

## Packages and releases

GitHub Actions builds every package (`.github/workflows/packages.yml`):

| Platform | Packages |
|:--|:--|
| Linux (x86_64, arm64) | `.deb`, `.rpm` (both install the udev rule), `.AppImage`: Electron |
| Windows (x64, arm64) | NSIS installer `-setup.exe`, `.msi` (x64 only), portable `.exe` (zip, needs WebView2) |
| macOS (universal: Apple silicon + Intel) | `.dmg`, `.app` (zip) |

Windows arm64 is cross-compiled on the x64 runner; Linux arm64 builds on GitHub's arm64
runner. The arm64 jobs may fail without blocking a release.

- **CI** (`ci.yml`): tests on every push and pull request (on the self-hosted runner;
  changes to only `*.md` / `docs/` skip it). The packages, as build artifacts, only when
  asked for: put `[package]` in the message of a commit pushed to `main`, or Actions → CI
  → Run workflow. `[skip ci]` in a commit message skips CI altogether.
- **Release** (`release.yml`): push a tag matching the version in `src-tauri/tauri.conf.json`
  (`git tag v0.2.0 && git push origin v0.2.0`); it builds everything and opens a **draft**
  GitHub Release with the packages and `SHA256SUMS.txt`, its notes from the version's
  section of `CHANGELOG.md` (write it before tagging). Review it, then publish.
  Each package is also there without the version in its name
  (`nebulosa-companion_x64-setup.exe`, …), so
  `https://github.com/Instrix/Instrix-Nebulosa-Companion/releases/latest/download/<that name>`
  always downloads the newest published release's (the website links to these).
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
