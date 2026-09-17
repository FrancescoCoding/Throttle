//! Throttle: a free per-process bandwidth monitor and limiter for Windows.
//!
//! This binary wires the WinDivert-based backend to the egui front-end over a
//! pair of crossbeam channels and launches the native window.

// Hide the extra console window on Windows release builds; keep it in debug so
// `tracing` output is visible while developing.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod backend;
mod gui;
mod icon;
mod single_instance;
mod types;

use crossbeam_channel::unbounded;

use types::Command;

fn main() -> eframe::Result {
    // Tooling: write the .ico asset (consumed by build.rs for the exe icon).
    // Runs before everything else: it needs neither the driver nor the lock.
    if std::env::args().any(|a| a == "--gen-icon") {
        let out = std::path::Path::new("assets/throttle.ico");
        if let Some(dir) = out.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(out, icon::ico_bytes()).expect("write ico");
        println!("wrote {}", out.display());
        return Ok(());
    }

    // Refuse to run twice: a second WinDivert shaper would double-capture
    // every packet and undermine the first one's limits. This runs before the
    // log file is opened, because opening it truncates the running instance's
    // log. The lock is held until the process exits.
    let _instance_lock = match single_instance::acquire() {
        Some(lock) => lock,
        None => return Ok(()),
    };

    setup_logging();

    // Panics in worker threads are otherwise invisible (no console): log them.
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        tracing::error!("PANIC in thread '{thread}': {info}");
    }));

    // GUI -> backend commands.
    let (cmd_tx, cmd_rx) = unbounded::<Command>();

    // Diagnostic mode: run the backend without any GUI for N seconds.
    if let Some(pos) = std::env::args().position(|a| a == "--headless") {
        let secs: u64 = std::env::args()
            .nth(pos + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(15);
        // Connectivity probe: TCP connects while the engine diverts, logging
        // success and latency.
        std::thread::spawn(|| {
            use std::net::{SocketAddr, TcpStream};
            let targets: [(&str, SocketAddr); 2] = [
                ("cloudflare", "1.1.1.1:443".parse().unwrap()),
                ("microsoft", "20.190.159.0:443".parse().unwrap()),
            ];
            loop {
                for (name, addr) in &targets {
                    let t0 = std::time::Instant::now();
                    match TcpStream::connect_timeout(addr, std::time::Duration::from_secs(3)) {
                        Ok(_) => {
                            tracing::info!("probe {name}: OK in {} ms", t0.elapsed().as_millis())
                        }
                        Err(e) => tracing::warn!("probe {name}: FAILED: {e}"),
                    }
                }
                // Also probe DNS (UDP path) via a std resolver lookup.
                let t0 = std::time::Instant::now();
                match std::net::ToSocketAddrs::to_socket_addrs(&"login.microsoftonline.com:443") {
                    Ok(_) => tracing::info!("probe dns: OK in {} ms", t0.elapsed().as_millis()),
                    Err(e) => tracing::warn!("probe dns: FAILED: {e}"),
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        });

        match backend::start(cmd_rx) {
            Ok(backend) => {
                tracing::info!("headless mode: backend running for {secs}s");
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
                while std::time::Instant::now() < deadline {
                    if let Ok(snap) = backend
                        .snapshot_rx
                        .recv_timeout(std::time::Duration::from_secs(2))
                    {
                        tracing::info!(
                            "snapshot: {} process(es), total down {} B/s, up {} B/s",
                            snap.processes.len(),
                            snap.total_down_rate,
                            snap.total_up_rate
                        );
                    }
                }
                let _ = cmd_tx.send(Command::Shutdown);
                // Let the aggregator persist rules before we exit.
                backend.join();
            }
            Err(e) => tracing::error!("headless: backend failed to start: {e:#}"),
        }
        return Ok(());
    }

    let app_icon = eframe::egui::IconData {
        rgba: icon::rgba(),
        width: icon::SIZE,
        height: icon::SIZE,
    };
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([720.0, 480.0])
            .with_icon(app_icon)
            .with_title("Throttle"),
        ..Default::default()
    };

    match backend::start(cmd_rx) {
        Ok(backend) => {
            // Only snapshot_rx is used by the GUI.
            let snapshot_rx = backend.snapshot_rx.clone();
            // `backend` stays alive for the duration of `run_native` and is
            // dropped only after the window closes, keeping the engine threads
            // running.
            tracing::info!("backend started; launching GUI");
            let result = eframe::run_native(
                "Throttle",
                native_options,
                Box::new(move |cc| Ok(Box::new(gui::ThrottleApp::new(cc, snapshot_rx, cmd_tx)))),
            );
            // The app sent `Command::Shutdown` on close (and dropping it dropped
            // the command sender). Wait for the aggregator to persist rules;
            // returning before it has written the file truncates it.
            backend.join();
            result
        }
        Err(err) => {
            tracing::error!("backend failed to start: {err:#}");
            let message = format!("{err:#}");
            // Show the error in a minimal window instead of exiting silently.
            eframe::run_native(
                "Throttle error",
                native_options,
                Box::new(move |_cc| Ok(Box::new(gui::ErrorApp::new(message.clone())))),
            )
        }
    }
}

fn setup_logging() {
    if cfg!(debug_assertions) {
        // Use the terminal in debug builds
        tracing_subscriber::fmt().init()
    } else {
        // Log to %APPDATA%\Throttle\throttle.log. The release binary has no
        // console, so a file is the only way to diagnose the driver/engine.
        let log_dir = std::env::var("APPDATA")
            .map(|a| std::path::PathBuf::from(a).join("Throttle"))
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        let _ = std::fs::create_dir_all(&log_dir);
        match std::fs::File::create(log_dir.join("throttle.log")) {
            Ok(file) => tracing_subscriber::fmt()
                .with_max_level(tracing::Level::INFO)
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
                .init(),
            Err(_) => tracing_subscriber::fmt()
                .with_max_level(tracing::Level::INFO)
                .init(),
        }
    }
}
