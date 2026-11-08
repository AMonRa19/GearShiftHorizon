use crate::telemetry::Telemetry;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path, time::{SystemTime, UNIX_EPOCH}};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CurvePoint { pub rpm_pct: f32, pub power_kw: f32, pub samples: u32 }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CarProfile {
    pub car_ordinal: i32, pub car_class: i32, pub pi: i32, pub drivetrain: i32,
    pub samples: u64, pub gear_ratio: [f32; 11], pub gear_samples: [u32; 11],
    pub upshift_rpm_pct: f32, pub downshift_rpm_pct: f32,
    pub upshift_observations: u32, pub downshift_observations: u32,
    pub power_curve: Vec<CurvePoint>, pub last_seen_unix: u64,
}
impl Default for CarProfile {
    fn default() -> Self { Self { car_ordinal:0, car_class:0, pi:0, drivetrain:0, samples:0,
        gear_ratio:[0.0;11], gear_samples:[0;11], upshift_rpm_pct:0.90, downshift_rpm_pct:0.38,
        upshift_observations:0, downshift_observations:0, power_curve:Vec::new(), last_seen_unix:0 } }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GraphPoint { pub time: u64, pub car_ordinal: i32, pub speed_kmh: f32, pub rpm_pct: f32, pub power_kw: f32, pub gear: u8 }

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CalibrationStore { pub profiles: BTreeMap<i32, CarProfile>, pub active_car: i32, pub history: Vec<GraphPoint> }
impl CalibrationStore {
    pub fn load() -> Self { fs::read(Path::new("tcu_calibration.json")).ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_default() }
    pub fn save(&self) { if let Ok(b)=serde_json::to_vec_pretty(self) { let _=fs::write("tcu_calibration.json",b); } }
    pub fn active(&self) -> Option<&CarProfile> { self.profiles.get(&self.active_car) }
    pub fn observe(&mut self, t:&Telemetry, previous:Option<&Telemetry>, learn_shift:bool) {
        if t.is_race_on==0 || t.car_ordinal<=0 || t.engine_max_rpm<=0.0 { return; }
        self.active_car=t.car_ordinal;
        let mut save_due=false;
        {
        let p=self.profiles.entry(t.car_ordinal).or_insert_with(||CarProfile{car_ordinal:t.car_ordinal,..CarProfile::default()});
        p.car_class=t.car_class; p.pi=t.pi; p.drivetrain=t.drivetrain; p.samples+=1;
        p.last_seen_unix=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let gear=t.gear as usize;
        if gear>0 && gear<11 && t.current_rpm>500.0 && t.speed_ms>5.0 && t.speed_ms.is_finite() {
            let ratio=t.speed_ms/t.current_rpm;
            if ratio.is_finite() && ratio>0.0001 && ratio<1.0 {
                let n=p.gear_samples[gear]; p.gear_ratio[gear]=if n==0 {ratio} else {p.gear_ratio[gear]*0.97+ratio*0.03};
                p.gear_samples[gear]=n.saturating_add(1);
            }
        }
        let rpm_pct=t.rpm_pct().clamp(0.0,1.2);
        let power_kw=(t.power_w/1000.0).max(0.0);
        if power_kw.is_finite() && rpm_pct>0.1 && t.throttle()>0.65 {
            let bucket=((rpm_pct*20.0).round() as i32).clamp(2,24) as f32/20.0;
            if let Some(pt)=p.power_curve.iter_mut().find(|x|(x.rpm_pct-bucket).abs()<0.026) {
                pt.power_kw=pt.power_kw*0.9+power_kw*0.1; pt.samples=pt.samples.saturating_add(1);
            } else { p.power_curve.push(CurvePoint{rpm_pct:bucket,power_kw,samples:1}); p.power_curve.sort_by(|a,b|a.rpm_pct.total_cmp(&b.rpm_pct)); }
            if p.power_curve.len()>24 { p.power_curve.remove(0); }
        }
        if learn_shift { if let Some(prev)=previous {
            if prev.car_ordinal==t.car_ordinal && prev.gear>0 && prev.gear<=10 && prev.gear!=t.gear {
                let before=prev.rpm_pct().clamp(0.0,1.2);
                if t.gear>prev.gear && prev.throttle()>0.45 && prev.brake()<0.1 {
                    let n=p.upshift_observations; p.upshift_rpm_pct=(if n==0 {before} else {p.upshift_rpm_pct*0.9+before*0.1}).clamp(0.65,0.98); p.upshift_observations=n.saturating_add(1);
                } else if t.gear<prev.gear && (prev.brake()>0.25 || prev.throttle()>0.55) {
                    let n=p.downshift_observations; p.downshift_rpm_pct=(if n==0 {before} else {p.downshift_rpm_pct*0.9+before*0.1}).clamp(0.18,0.68); p.downshift_observations=n.saturating_add(1);
                }
            }
        }}
        save_due=p.samples%30==0;
        }
        self.history.push(GraphPoint{time:SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),car_ordinal:t.car_ordinal,speed_kmh:t.speed_kmh(),rpm_pct,power_kw,gear:t.gear});
        if self.history.len()>240 { self.history.drain(0..self.history.len()-240); }
        if save_due { self.save(); }
    }
    pub fn reset_active(&mut self) { let id=self.active_car; if let Some(p)=self.profiles.get_mut(&id) { *p=CarProfile{car_ordinal:id,..CarProfile::default()}; } self.save(); }
}
