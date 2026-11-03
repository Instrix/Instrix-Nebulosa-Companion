# Hardware test

What to check with a real Nebulosa board before a release, and what each step
should show. The automated part comes first; the rest needs a person, a board
and a load.

## 1. Automated soak (no window)

`examples/soak.rs` drives the desktop app's own backend: it connects, streams,
records (`.nbrec`), logs (`.nblog`) and exports (CSV of every sample, then
HDF5 of the summaries) at the same time, then reads every file back.

```sh
# against the simulator
../Instrix-Nebulosa-Debugger/build/nebulosa-sim &
cargo run --release -p nebulosa-companion --example soak -- 127.0.0.1 60

# against a board: USB (the first one found), or Ethernet
cargo run --release -p nebulosa-companion --example soak -- usb 600
cargo run --release -p nebulosa-companion --example soak -- 192.168.1.50 600

# arguments: target, seconds, log2 of the decimation (0 = full rate), output folder
```

It ends with `ALL PASS` (exit 0) or the failed checks (exit 1):

| Check | Means |
|---|---|
| rate | samples arrived at the expected rate (≥ 97 %) |
| no lost blocks / stalls / malformed | the link and the firmware kept up |
| memory steady | RSS moved less than 50 MB after the first 5 s |
| `.nbrec` read back | every sample recorded, and the file parses |
| `.nblog` | complete, ~10 entries a second (100 ms) |
| CSV | one row per sample of its phase; the last row parses |
| HDF5 | the rows written; charge never decreases |

Run it for 10 minutes on USB SuperSpeed and on Ethernet at full rate, and for
10 minutes on USB Full-Speed at log2n 4 (125 kS/s; Full-Speed carries ~1 MB/s).
The simulator passes it at 2 MS/s with all four outputs at once.

## 2. By hand, in the app (`npm run dev`)

Connect the board, source **ADC**, a load you know (a resistor, or a board
with a known sleep / wake current).

### Link and stream
- [ ] USB SuperSpeed and Ethernet both appear (Refresh); connecting shows the
      firmware, the FPGA ID and the link.
- [ ] N = 1 (2 MS/s): the footer's lost / stalls / OVF / seq err stay 0 for
      5 minutes; MB/s ≈ 8.
- [ ] Unplug USB while streaming: "Disconnected", no crash; plug back,
      connect again.

### Scope
- [ ] Zoom from 60 s/div to 1 µs/div with the wheel: smooth; at µs a dot per sample.
- [ ] A known DC current reads right in the meter (mean), in each range.
- [ ] Auto range: the plot shows the range switches without gaps; dirty samples
      only around switches.
- [ ] Autoset on a periodic load: a stable picture, the period right.
- [ ] Trigger: edge, pulse width (a known pulse), window, timeout; Normal / Single.
      The event list's width and ∫ match what you expect of the load.
- [ ] Drag the level line; drag a dual marker; its ∫I matches the meter's charge
      over the same span.
- [ ] Math channel: `V/I` on a resistor gives its resistance.

### Logger and files
- [ ] Logger view: follow, zoom, click a moment: it opens in the scope.
- [ ] Log 1 hour to CSV (per 100 ms) and to HDF5; open the CSV in a
      spreadsheet and the HDF5 in Python (`h5py`): the values match the meter.
- [ ] Log every sample to CSV for 1 minute: no drops reported; size ≈ 60 MB/s.
- [ ] REC 1 minute; open the recording: same picture, same statistics.
- [ ] Fill a small disk (or set a low raw limit): logging stops with a message,
      the files read back.

### Long run
- [ ] Logger, 100 ms, `.nblog` + raw off, 24 hours: the app's memory stays
      under ~400 MB (the session log is bounded); the log opens complete.

## Known gaps

- Calibration: the voltage scale is the firmware's debug 1 mV/code.
- Windows needs WinUSB bound to the device (Zadig) until the firmware has
  MS OS 2.0 descriptors; the browser build needs them too.
