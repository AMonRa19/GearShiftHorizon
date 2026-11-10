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
    /// Smoothed per-car engine_max_rpm reported by FH telemetry; not a dyno-measured limiter cut.
    pub learned_redline_rpm: f32, pub redline_observations: u32,
    pub power_curve: Vec<CurvePoint>, pub last_seen_unix: u64,
    /// Online acceleration model learned only from clean, steady-gear pulls.
    pub gear_accel_ema: [f32; 11], pub gear_accel_samples: [u32; 11],
    pub drive_style_aggression: f32, pub learning_confidence: f32,
}
impl Default for CarProfile {
    fn default() -> Self { Self { car_ordinal:0, car_class:0, pi:0, drivetrain:0, samples:0,
        gear_ratio:[0.0;11], gear_samples:[0;11], upshift_rpm_pct:0.90, downshift_rpm_pct:0.38,
        upshift_observations:0, downshift_observations:0, learned_redline_rpm:0.0, redline_observations:0, power_curve:Vec::new(), last_seen_unix:0,
        gear_accel_ema:[0.0;11], gear_accel_samples:[0;11], drive_style_aggression:0.5, learning_confidence:0.0 } }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GraphPoint { pub time: u64, pub car_ordinal: i32, pub speed_kmh: f32, pub rpm_pct: f32, pub power_kw: f32, pub gear: u8 }

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CalibrationStore { pub profiles: BTreeMap<i32, CarProfile>, pub active_car: i32, pub history: Vec<GraphPoint> }

fn cfg_power_sample(power_kw: f32, rpm_pct: f32, t: &Telemetry, clean_pull: bool) -> bool {
    clean_pull && power_kw.is_finite() && power_kw > 1.0 && rpm_pct > 0.15 && rpm_pct < 1.05
        && t.current_rpm > 900.0
}

impl CalibrationStore {
    pub fn load() -> Self { fs::read(Path::new("tcu_calibration.json")).ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_default() }
    pub fn save(&self) { if let Ok(b)=serde_json::to_vec_pretty(self) { let _=fs::write("tcu_calibration.json",b); } }
    pub fn active(&self) -> Option<&CarProfile> { self.profiles.get(&self.active_car) }
}

impl CarProfile {
    /// Returns an interpolated power estimate only after the curve has enough
    /// clean observations to be useful for shift selection.
    pub fn estimated_power_kw(&self, rpm_pct: f32) -> Option<f32> {
        // No heap allocation in the control path: walk the sorted curve directly.
        let x = rpm_pct.clamp(0.10, 1.05);
        let mut first: Option<&CurvePoint> = None;
        let mut previous: Option<&CurvePoint> = None;
        let mut count = 0usize;
        for point in self.power_curve.iter().filter(|p| p.samples >= 12 && p.power_kw.is_finite() && p.power_kw > 0.0) {
            count += 1;
            if first.is_none() { first = Some(point); }
            if let Some(a) = previous {
                if x <= point.rpm_pct {
                    let span = (point.rpm_pct - a.rpm_pct).max(0.001);
                    let f = ((x - a.rpm_pct) / span).clamp(0.0, 1.0);
                    return Some(a.power_kw + (point.power_kw - a.power_kw) * f);
                }
            }
            previous = Some(point);
        }
        if count < 3 { return None; }
        let first = first?;
        let last = previous?;
        if x <= first.rpm_pct { Some(first.power_kw) } else { Some(last.power_kw) }
    }

    /// Decide whether the next gear produces more estimated wheel-force proxy.
    /// Returns None until both gear ratios and a useful power curve are learned.
    pub fn should_upshift(&self, speed_ms: f32, current_rpm: f32, engine_max_rpm: f32, current_gear: u8) -> Option<bool> {
        if !speed_ms.is_finite() || speed_ms < 2.0 || !current_rpm.is_finite() || engine_max_rpm <= 0.0 || current_gear == 0 || current_gear >= 10 { return None; }
        let current = current_gear as usize;
        let next = current + 1;
        if self.gear_samples[current] < 30 || self.gear_samples[next] < 30 { return None; }
        let current_ratio = self.gear_ratio[current];
        let next_ratio = self.gear_ratio[next];
        if !current_ratio.is_finite() || !next_ratio.is_finite() || current_ratio <= 0.0 || next_ratio <= 0.0 || next_ratio >= current_ratio { return None; }
        let next_rpm = speed_ms * next_ratio;
        let current_pct = (current_rpm / engine_max_rpm).clamp(0.10, 1.05);
        let next_pct = next_rpm / engine_max_rpm;
        if !next_pct.is_finite() || !(0.22..=0.94).contains(&next_pct) { return None; }
        if self.power_curve.iter().filter(|p| p.samples >= 12 && p.power_kw.is_finite() && p.power_kw > 0.0).count() < 3 { return None; }
        let current_power = self.estimated_power_kw(current_pct)?;
        let next_power = self.estimated_power_kw(next_pct)?;
        let current_omega = current_rpm * std::f32::consts::TAU / 60.0;
        let next_omega = next_rpm * std::f32::consts::TAU / 60.0;
        if current_omega <= 1.0 || next_omega <= 1.0 { return None; }
        let current_force_proxy = (current_power * 1000.0 / current_omega) * current_ratio;
        let next_force_proxy = (next_power * 1000.0 / next_omega) * next_ratio;
        if !current_force_proxy.is_finite() || !next_force_proxy.is_finite() { return None; }
        Some(next_force_proxy > current_force_proxy * 1.025)
    }

    /// Pick a lower gear using learned RPM/speed ratios and the learned power
    /// curve. Returns None until the data quality is sufficient.
    pub fn best_lower_gear(&self, speed_ms: f32, engine_max_rpm: f32, current_gear: u8) -> Option<u8> {
        if !speed_ms.is_finite() || speed_ms < 2.0 || engine_max_rpm <= 0.0 { return None; }
        let mut best: Option<(u8, f32)> = None;
        for gear in 1..current_gear.min(11) as usize {
            if self.gear_samples[gear] < 30 { continue; }
            let ratio = self.gear_ratio[gear];
            let predicted_rpm = speed_ms * ratio;
            let rpm_pct = predicted_rpm / engine_max_rpm;
            if !rpm_pct.is_finite() || !(0.38..=0.90).contains(&rpm_pct) { continue; }
            let power_kw = self.estimated_power_kw(rpm_pct)?;
            let omega = predicted_rpm * std::f32::consts::TAU / 60.0;
            if omega <= 1.0 { continue; }
            let engine_torque = power_kw * 1000.0 / omega;
            // Ratio is proportional to total reduction; this is a wheel-force
            // proxy, not an absolute drivetrain torque measurement.
            let mut score = engine_torque * ratio;
            if rpm_pct > 0.84 { score *= 1.0 - (rpm_pct - 0.84) * 2.0; }
            if best.map(|(_, old)| score > old).unwrap_or(true) { best = Some((gear as u8, score)); }
        }
        best.map(|(gear, _)| gear)
    }
}

impl CalibrationStore {
    pub fn observe(&mut self, t:&Telemetry, previous:Option<&Telemetry>, learn_shift:bool) {
        if t.is_race_on==0 || t.car_ordinal<=0 || t.engine_max_rpm<=0.0 { return; }
        self.active_car=t.car_ordinal;
        let mut save_due=false;
        let rpm_pct=t.rpm_pct().clamp(0.0,1.2);
        let power_kw=(t.power_w/1000.0).max(0.0);
        let mut measured_accel = None;
        if let Some(prev) = previous {
            if prev.car_ordinal == t.car_ordinal && prev.gear == t.gear && prev.gear > 0 {
                let dt_ms = t.session_timestamp.wrapping_sub(prev.session_timestamp);
                let dt = dt_ms as f32 / 1000.0;
                if (0.005..=0.15).contains(&dt) {
                    let dv = t.speed_ms - prev.speed_ms;
                    if dv.is_finite() { measured_accel = Some((dv / dt).clamp(-25.0, 25.0)); }
                }
            }
        }
        {
            let p=self.profiles.entry(t.car_ordinal).or_insert_with(||CarProfile{car_ordinal:t.car_ordinal,..CarProfile::default()});
            p.car_class=t.car_class; p.pi=t.pi; p.drivetrain=t.drivetrain; p.samples+=1;
            // Learn the per-car maximum RPM reported by FH telemetry using a smoothed estimate.
            if t.engine_max_rpm.is_finite() && (3000.0..=20000.0).contains(&t.engine_max_rpm) {
                let n = p.redline_observations;
                p.learned_redline_rpm = if n == 0 { t.engine_max_rpm } else { p.learned_redline_rpm * 0.92 + t.engine_max_rpm * 0.08 };
                p.redline_observations = n.saturating_add(1);
            }
            p.last_seen_unix=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
            let gear=t.gear as usize;
            let stable_gear = previous.map(|prev| prev.car_ordinal == t.car_ordinal && prev.gear == t.gear).unwrap_or(false);
            let low_slip = t.rear_slip() < 0.22 && t.slip_fl.abs() < 0.22 && t.slip_fr.abs() < 0.22;
            let straight = t.accel_x.abs() < 0.85;
            let clean_pull = learn_shift && stable_gear && t.throttle() > 0.62 && t.brake() < 0.03
                && low_slip && straight && t.speed_ms > 5.0;

            // Learn per-gear RPM/speed ratios only from stable, straight,
            // low-slip samples. This prevents wheelspin and cornering from
            // corrupting the gear model.
            if learn_shift && gear>0 && gear<11 && stable_gear && t.current_rpm>900.0 && t.speed_ms>8.0
                && t.speed_ms.is_finite() && t.brake()<0.08 && low_slip && straight {
                if p.gear_samples[gear] > 0 && p.gear_ratio[gear] < 1.0 {
                    p.gear_ratio[gear] = 0.0; p.gear_samples[gear] = 0;
                }
                let ratio=t.current_rpm/t.speed_ms;
                if ratio.is_finite() && ratio>1.0 && ratio<2000.0 {
                    let n=p.gear_samples[gear];
                    let alpha = if n < 8 { 0.30 } else { 0.10 };
                    p.gear_ratio[gear]=if n==0 {ratio} else {p.gear_ratio[gear]*(1.0-alpha)+ratio*alpha};
                    p.gear_samples[gear]=n.saturating_add(1);
                }
            }

            // Learn acceleration response per gear, not just RPM thresholds.
            if clean_pull && gear>0 && gear<11 {
                if let Some(a) = measured_accel.filter(|a| *a > 0.15 && *a < 12.0) {
                    let n=p.gear_accel_samples[gear];
                    p.gear_accel_ema[gear]=if n==0 { a } else { p.gear_accel_ema[gear]*0.88+a*0.12 };
                    p.gear_accel_samples[gear]=n.saturating_add(1);
                }
            }

            // Build the power curve only from meaningful, low-slip loaded pulls.
            if cfg_power_sample(power_kw, rpm_pct, t, clean_pull) {
                let bucket=((rpm_pct*20.0).round() as i32).clamp(2,22) as f32/20.0;
                if let Some(pt)=p.power_curve.iter_mut().find(|x|(x.rpm_pct-bucket).abs()<0.026) {
                    let alpha = if pt.samples < 5 { 0.28 } else { 0.10 };
                    pt.power_kw=pt.power_kw*(1.0-alpha)+power_kw*alpha;
                    pt.samples=pt.samples.saturating_add(1);
                } else {
                    p.power_curve.push(CurvePoint{rpm_pct:bucket,power_kw,samples:1});
                    p.power_curve.sort_by(|a,b|a.rpm_pct.total_cmp(&b.rpm_pct));
                }
                if p.power_curve.len()>24 { p.power_curve.remove(0); }
            }

            // Adaptive drive-style score: higher values mean more aggressive
            // throttle use; braking and gentle cruising pull it down.
            let style_sample = (t.throttle()*0.78 + (t.brake()*0.35) + if t.speed_ms > 15.0 {0.05} else {0.0}).clamp(0.0,1.0);
            p.drive_style_aggression = p.drive_style_aggression*0.985 + style_sample*0.015;

            if learn_shift { if let Some(prev)=previous {
                if prev.car_ordinal==t.car_ordinal && prev.gear>0 && prev.gear<=10 && prev.gear!=t.gear {
                    let before=prev.rpm_pct().clamp(0.0,1.2);
                    // Ignore shifts during braking, wheelspin, airtime or a likely collision.
                    let clean_shift = prev.rear_slip() < 0.25 && prev.accel_x.abs() < 0.9
                        && prev.brake() < 0.15 ;
                    if clean_shift && t.gear>prev.gear && prev.throttle()>0.45 {
                        let n=p.upshift_observations;
                        p.upshift_rpm_pct=(if n==0 {before} else {p.upshift_rpm_pct*0.90+before*0.10}).clamp(0.68,0.96);
                        p.upshift_observations=n.saturating_add(1);
                    } else if clean_shift && t.gear<prev.gear && prev.throttle()>0.45 {
                        let n=p.downshift_observations;
                        p.downshift_rpm_pct=(if n==0 {before} else {p.downshift_rpm_pct*0.90+before*0.10}).clamp(0.20,0.66);
                        p.downshift_observations=n.saturating_add(1);
                    }
                }
            }}
            let ratio_coverage = p.gear_samples.iter().filter(|n| **n >= 30).count() as f32 / 10.0;
            let power_coverage = (p.power_curve.iter().filter(|x| x.samples >= 12).count() as f32 / 8.0).clamp(0.0,1.0);
            let accel_coverage = p.gear_accel_samples.iter().filter(|n| **n >= 20).count() as f32 / 10.0;
            p.learning_confidence = (ratio_coverage*0.45 + power_coverage*0.35 + accel_coverage*0.20).clamp(0.0,1.0);
            save_due=p.samples%300==0;
        }
        self.history.push(GraphPoint{time:SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),car_ordinal:t.car_ordinal,speed_kmh:t.speed_kmh(),rpm_pct,power_kw,gear:t.gear});
        if self.history.len()>240 { self.history.drain(0..self.history.len()-240); }
        if save_due { self.save(); }
    }
    pub fn reset_active(&mut self) { let id=self.active_car; if let Some(p)=self.profiles.get_mut(&id) { *p=CarProfile{car_ordinal:id,..CarProfile::default()}; } self.save(); }
}

#[cfg(test)]
mod learning_tests {
    use super::*;

    #[test]
    fn power_curve_interpolates_after_enough_samples() {
        let mut p = CarProfile::default();
        p.power_curve = vec![
            CurvePoint { rpm_pct: 0.50, power_kw: 200.0, samples: 12 },
            CurvePoint { rpm_pct: 0.70, power_kw: 300.0, samples: 12 },
            CurvePoint { rpm_pct: 0.90, power_kw: 250.0, samples: 12 },
        ];
        let estimate = p.estimated_power_kw(0.60).unwrap();
        assert!((estimate - 250.0).abs() < 0.1);
    }

    #[test]
    fn best_lower_gear_uses_learned_ratios_and_power_curve() {
        let mut p = CarProfile::default();
        p.gear_ratio[2] = 250.0; p.gear_samples[2] = 30;
        p.gear_ratio[3] = 190.0; p.gear_samples[3] = 30;
        p.gear_ratio[4] = 145.0; p.gear_samples[4] = 30;
        p.power_curve = vec![
            CurvePoint { rpm_pct: 0.40, power_kw: 180.0, samples: 12 },
            CurvePoint { rpm_pct: 0.60, power_kw: 280.0, samples: 12 },
            CurvePoint { rpm_pct: 0.80, power_kw: 320.0, samples: 12 },
            CurvePoint { rpm_pct: 0.90, power_kw: 290.0, samples: 12 },
        ];
        let target = p.best_lower_gear(20.0, 7000.0, 5);
        assert!(target.map(|g| g < 5 && g >= 2).unwrap_or(false));
    }
}
