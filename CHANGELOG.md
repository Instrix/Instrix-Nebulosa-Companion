# Changelog

What changed in each release of Nebulosa Companion. The release workflow
(`.github/workflows/release.yml`) puts a version's section at the top of its
GitHub Release.

## [0.3.0] - 2026-11-07

### Added

- **Settings window** (⚙ in the header, or Ctrl + ,): theme, UI scale, the side
  panels and their sizes, updates, beta features, remote control, and the way to
  Configurations and the walkthrough.
- **Three more dark themes**: Charcoal (neutral grey), OLED black (true black) and
  High contrast (black and white, bright edges, a yellow focus ring). "The system's"
  also follows the OS's high-contrast setting.
- **Update check**: on start (desktop app; Settings can turn it off) and from
  Help → Check for updates. Shows the release notes and the download for this
  computer (Windows / macOS / Linux, x64 or ARM); "Skip this version" keeps it quiet.
- **Remote control** (Settings → Remote control): the Companion in a browser on a
  port of this computer, or of the local network. The browser mirrors the app's
  window: the same device or recording, the instrument's settings and streaming in
  step both ways, each its own view. Protected by a token; never expose it to the
  internet.
- **Remote API**: JSON over HTTP on the same port (`/api/v1/<action>`): status,
  devices, connect, demo, disconnect, start, stop, measure, value, settings, set,
  markers, marker, clear_markers, record, log, mode, screenshot. See the README.
- **Debug tab** (a build option: `npm run dev:debug`, `build:debug`, `linux:debug`,
  `linux:dist:debug`, or `NEBULOSA_DEBUG=1`; left out of the normal builds and the
  releases): the Nebulosa Debugger's functions.
  - Tools: the FPGA registers, the range switch's timing and forced ranges, the
    RS1473 ADC link (lock, RECAL, phases, phase sweep, echo engine), the board's
    low-level controls, the SPI flash, raw NBP commands, and a log to copy a line
    or all of it.
  - AFE bring-up: the AFE1 ×10 bring-up with a Siglent SPD3303C and a Korad KEL102
    (USBTMC, serial or LAN): the playbook, interlocks and ALL OFF, the bench, the
    sequences (zero and noise, linearity, ladder ratio, comparator trip points),
    the results as CSV. A simulated bench ("sim") runs it all on the demo device.
  - The side panels are hidden in the Debug tab (◧ / ◨ bring them back).
- **Beta features** in Settings: features that may change, off until turned on.

### Changed

- **Side-channel is a beta feature**: its tab, Ctrl + 3, its tutorial and its
  showcase are hidden until Settings → Beta features → Side-channel analysis is on.
- The ⚙ button opens Settings; Configurations is one click inside (also in the
  Help and Aa menus, and Ctrl K).
- Help (?) has "Check for updates…"; the Aa menu lists every theme.
- The desktop app opens web links in the system's browser (Windows and macOS too).
- Releases also carry each package without the version in its name
  (`nebulosa-companion_x64-setup.exe`, …), so
  `releases/latest/download/<name>` always gives the newest release's.

### CI

- The packages for every platform (on GitHub's billed runners) are built only when
  asked for: a commit on `main` with `[package]` in its message, or Run workflow.
  Pushes to `main` run the tests only, on the self-hosted runner.
- Changes to documentation only (`*.md`, `docs/`) skip CI.
- CI checks that the Debug tab is in the debug build and absent from the normal one.
- A release's notes come from this file.

## [0.2.1] - 2026-11-06

See the [GitHub release](https://github.com/Instrix/Instrix-Nebulosa-Companion/releases/tag/v0.2.1).
