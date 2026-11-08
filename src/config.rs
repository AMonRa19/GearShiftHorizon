use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use crate::paths;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Features {
    pub cornering_lock: bool,
    pub brake_curve: bool,
    pub shift_advisor_manual: bool,
    pub power_curve_learning: bool,
    pub airtime_lock: bool,
    pub drive_style_adaptive: bool,
    pub launch_control: bool,
    pub drivetrain_aware: bool,
    pub engine_brake: bool,
    pub turbo_lag_compensation: bool,
    pub transient_lock: bool,
}
impl Default for Features {
    fn default() -> Self { Self {
        cornering_lock:true, brake_curve:true, shift_advisor_manual:true,
        power_curve_learning:true, airtime_lock:true, drive_style_adaptive:true,
        launch_control:true, drivetrain_aware:true, engine_brake:true,
        turbo_lag_compensation:true, transient_lock:true,
    }}
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub udp_port: u16,
    pub key_up: String,
    pub key_down: String,
    pub shift_hold_ms: u64,
    pub shift_gap_ms: u64,
    pub mode: String,
    pub automatic: bool,
    pub upshift_rpm_pct: f32,
    pub downshift_rpm_pct: f32,
    pub brake_downshift: bool,
    pub kickdown_throttle: f32,
    pub kickdown_rpm_pct: f32,
    pub min_shift_interval_ms: u64,
    /// User-adjustable delay between automatic shift commands.
    pub shift_delay_ms: u64,
    pub web_port: u16,
    pub features: Features,
}
impl Default for Config {
    fn default() -> Self {
        Self { udp_port:5555, key_up:"e".into(), key_down:"q".into(),
            shift_hold_ms:65, shift_gap_ms:60, mode:"Race".into(), automatic:true,
            upshift_rpm_pct:0.90, downshift_rpm_pct:0.38, brake_downshift:true,
            kickdown_throttle:0.78, kickdown_rpm_pct:0.58, min_shift_interval_ms:300, shift_delay_ms:300, web_port:8765, features:Features::default() }
    }
}
impl Config {
    pub fn load() -> Result<Self> {
        let p = paths::data_file("tcu_config.json");
        if p.exists() { Ok(serde_json::from_slice(&fs::read(p)?)?) }
        else { let c=Self::default(); c.save()?; Ok(c) }
    }
    pub fn save(&self) -> Result<()> {
        fs::write(paths::data_file("tcu_config.json"), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}
