# Instrix-Nebulosa-Companion

Companion app for the INSTRIX Nebulosa power profiler
([Instrix-Nebulosa-Firmware](../Instrix-Nebulosa-Firmware)). It talks NBP (the
Nebulosa Bulk Protocol) over USB or Ethernet, and runs as a desktop app
(Windows, Linux, macOS) or in a browser.

## Layout

```
crates/nbp-core/   Rust, no I/O: NBP framing, messages, format-5 stream decoder, display
                   reduction. Built natively for the desktop app and to WASM for the UI.
src-tauri/         desktop shell (Tauri 2): USB (nusb), TCP, UDP discovery
ui/                Svelte 5 + uPlot. One Device API over two transports:
                   lib/native.ts (Tauri backend) and lib/webusb.ts (browser)
udev/              Linux permissions for the USB devices
```

```
UI (Svelte) ── Device ──┬── native.ts ── Tauri IPC ── src-tauri: nusb / tokio TCP ──┐
                        └── webusb.ts ── WebUSB ──────────────────────────────────┐ │
                                                                  nbp-core (WASM) ┘ └ nbp-core (native)
```

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

# Linux: Tauri's system libraries
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libayatana-appindicator3-dev librsvg2-dev build-essential pkg-config
# Linux: USB access without root
sudo cp udev/99-nebulosa.rules /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger

npm install
```

## Run

```sh
npm run dev        # desktop app (USB, Ethernet, discovery)
npm run web        # browser build at http://localhost:5173 (Chrome / Edge: WebUSB)
npm run build      # desktop installers (src-tauri/target/release/bundle)
npm run build:ui   # static browser build in dist/
npm test           # nbp-core unit tests
```

Without hardware: run the debugger's simulator
(`../Instrix-Nebulosa-Debugger/build/nebulosa-sim`) and connect to `127.0.0.1`.

## Platform notes

- **Browser:** WebUSB works only in Chromium-based browsers. Browsers have no raw TCP/UDP, so
  the browser build is USB only. Ethernet would need a WebSocket endpoint in the firmware.
- **Windows:** the NBP interface must bind WinUSB, both for the desktop app (nusb) and for
  WebUSB. That needs MS OS 2.0 descriptors in the firmware (to do), or Zadig until then.
- **VID/PID:** `1A86` is WCH's vendor ID. A product release needs its own.
- **Units:** the plot shows raw 14-bit codes per range. Calibration to amps and volts is to do.
- **Stream format:** only format 5 (FPGA ID `0x4E42000B` and later) is decoded.
