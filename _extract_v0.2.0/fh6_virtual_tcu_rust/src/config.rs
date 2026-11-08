use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

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
}
impl Default for Config {
    fn default() -> Self {
        Self { udp_port:5555, key_up:"e".into(), key_down:"q".into(),
            shift_hold_ms:65, shift_gap_ms:60, mode:"Race".into(), automatic:true,
            upshift_rpm_pct:0.90, downshift_rpm_pct:0.38, brake_downshift:true,
            kickdown_throttle:0.78, kickdown_rpm_pct:0.58, min_shift_interval_ms:300 }
    }
}
impl Config {
    pub fn load() -> Result<Self> {
        let p=Path::new("tcu_config.json");
        if p.exists() { Ok(serde_json::from_slice(&fs::read(p)?)?) }
        else { let c=Self::default(); c.save()?; Ok(c) }
    }
    pub fn save(&self) -> Result<()> {
        fs::write("tcu_config.json", serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}
