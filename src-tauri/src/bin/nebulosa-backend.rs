//! The backend alone: serves the UI (`--dist`) and its WebSocket on
//! 127.0.0.1, for a Chromium window (the Linux desktop app, electron/).
//!
//!   nebulosa-backend --dist <ui folder> [--port N] [--exit-with-stdin]
//!
//! Prints `NEBULOSA_BACKEND <url>` when ready. `--exit-with-stdin`: quits when
//! stdin closes (the shell that started it is gone, however it ended).

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut dist, mut port, mut with_stdin) = (None::<PathBuf>, 0u16, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dist" => dist = args.next().map(PathBuf::from),
            "--port" => port = args.next().and_then(|p| p.parse().ok()).unwrap_or(0),
            "--exit-with-stdin" => with_stdin = true,
            _ => {
                eprintln!("usage: nebulosa-backend --dist <ui folder> [--port N] [--exit-with-stdin]");
                std::process::exit(2);
            }
        }
    }
    let dist = dist.unwrap_or_else(|| {
        // next to the executable, or the repo's build
        let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_default();
        [exe.join("dist"), exe.join("../../dist")].into_iter().find(|d| d.join("index.html").exists()).unwrap_or_else(|| PathBuf::from("dist"))
    });
    if with_stdin {
        std::thread::spawn(|| {
            let mut sink = Vec::new();
            let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut sink);
            std::process::exit(0);
        });
    }
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
    if let Err(e) = rt.block_on(nebulosa_companion_lib::server::serve(dist, port)) {
        eprintln!("nebulosa-backend: {e}");
        std::process::exit(1);
    }
}
