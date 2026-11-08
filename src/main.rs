mod config;
mod input;
mod logic;
pub mod telemetry;
mod ui;
mod web;

use anyhow::Result;
use config::Config;
use crossbeam_channel::unbounded;
use log::{error, info};
use std::{net::UdpSocket, sync::{Arc, RwLock}, thread, time::{Duration, Instant}};
use telemetry::Telemetry;

fn main() -> Result<()> {
    env_logger::init();
    let cfg = Arc::new(RwLock::new(Config::load()?));
    web::start(Arc::clone(&cfg));
    let (tx, rx) = unbounded::<Telemetry>();
    let (cmd_tx, cmd_rx) = unbounded::<input::ShiftCommand>();

    let cfg_net = Arc::clone(&cfg);
    thread::Builder::new().name("fh6-udp".into()).spawn(move || {
        let port = cfg_net.read().unwrap().udp_port;
        let socket = match UdpSocket::bind(("0.0.0.0", port)) {
            Ok(s) => s,
            Err(e) => { error!("Не удалось открыть UDP {}: {}", port, e); return; }
        };
        let _ = socket.set_read_timeout(Some(Duration::from_millis(250)));
        info!("Ожидание телеметрии FH6 на UDP {}", port);
        let mut buf = [0u8; 2048];
        loop {
            match socket.recv_from(&mut buf) {
                Ok((n, _)) => if let Some(td) = Telemetry::parse(&buf[..n]) {
                    let _ = tx.send(td);
                },
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {},
                Err(e) => error!("UDP receive: {}", e),
            }
        }
    })?;

    let cfg_keys = Arc::clone(&cfg);
    thread::Builder::new().name("keyboard-output".into()).spawn(move || {
        input::run_keyboard_worker(cmd_rx, cfg_keys);
    })?;

    let cfg_logic = Arc::clone(&cfg);
    let cmd_logic = cmd_tx.clone();
    thread::Builder::new().name("tcu-logic".into()).spawn(move || {
        let mut tcu = logic::Tcu::default();
        let mut last = Instant::now();
        loop {
            if let Ok(td) = rx.recv_timeout(Duration::from_millis(100)) {
                let now = Instant::now();
                let dt = now.duration_since(last).as_secs_f32().clamp(0.001, 0.2);
                last = now;
                let settings = cfg_logic.read().unwrap().clone();
                if let Some(cmd) = tcu.process(&td, &settings, dt) {
                    let _ = cmd_logic.send(cmd);
                }
                ui::publish_snapshot(td);
            } else {
                tcu.on_telemetry_timeout();
            }
        }
    })?;

    let native = eframe::NativeOptions::default();
    eframe::run_native(
        "FH6 Virtual TCU — Rust",
        native,
        Box::new(move |_cc| Box::new(ui::TcuApp::new(Arc::clone(&cfg)))),
    ).map_err(|e| anyhow::anyhow!("GUI: {e}"))?;
    Ok(())
}
