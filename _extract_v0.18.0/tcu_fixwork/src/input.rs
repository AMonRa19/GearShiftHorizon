use crate::config::Config;
use crossbeam_channel::Receiver;
use log::{error, info};
use rdev::{simulate, EventType, Key};
use std::{sync::{Arc,RwLock}, thread, time::Duration};

// Upstream Virtual TCU KEY_HOLD_S = 0.04 seconds.
const KEY_HOLD_MS: u64 = 40;

#[derive(Clone, Debug)]
pub struct ShiftCommand { pub from: u8, pub to: u8, pub reason: &'static str }

fn parse_key(s: &str) -> Option<Key> {
    match s.to_lowercase().as_str() {
        "q"=>Some(Key::KeyQ), "e"=>Some(Key::KeyE), "r"=>Some(Key::KeyR),
        "f"=>Some(Key::KeyF), "shift"=>Some(Key::ShiftLeft),
        "1"=>Some(Key::Num1), "2"=>Some(Key::Num2), _=>None
    }
}
fn key_event(key: Key, down: bool) -> Result<(),String> {
    let ev=if down {EventType::KeyPress(key)} else {EventType::KeyRelease(key)};
    simulate(&ev).map_err(|e| format!("{e:?}"))
}
pub fn run_keyboard_worker(rx: Receiver<ShiftCommand>, cfg: Arc<RwLock<Config>>) {
    info!("Keyboard worker ready");
    while let Ok(cmd)=rx.recv() {
        if cmd.from==cmd.to { continue; }
        let c=cfg.read().unwrap().clone();
        let key=if cmd.to>cmd.from { &c.key_up } else { &c.key_down };
        let Some(k)=parse_key(key) else { error!("Unsupported key binding: {key}"); continue; };
        let count=cmd.to.abs_diff(cmd.from);
        for _ in 0..count {
            if let Err(e)=key_event(k,true) { error!("Key down failed: {e}"); break; }
            thread::sleep(Duration::from_millis(KEY_HOLD_MS));
            if let Err(e)=key_event(k,false) { error!("Key release failed: {e}"); break; }
            thread::sleep(Duration::from_millis(c.shift_gap_ms));
        }
    }
}
