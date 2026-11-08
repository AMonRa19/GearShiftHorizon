use crate::{calibration::CarProfile, config::Config, input::ShiftCommand, telemetry::Telemetry};
use std::time::Instant;

#[derive(Default)]
pub struct Tcu {
    last_shift: Option<Instant>,
    last_timestamp: Option<u32>,
    telemetry_lost: bool,
}
impl Tcu {
    pub fn on_telemetry_timeout(&mut self) { self.telemetry_lost=true; }
    fn can_shift(&self, cfg:&Config) -> bool {
        !self.telemetry_lost && self.last_shift.map(|t| t.elapsed().as_millis() >= cfg.min_shift_interval_ms as u128).unwrap_or(true)
    }
    fn issue(&mut self, td:&Telemetry, target:u8, reason:&'static str) -> Option<ShiftCommand> {
        if target==td.gear { return None; }
        self.last_shift=Some(Instant::now());
        Some(ShiftCommand{from:td.gear,to:target,reason})
    }
    pub fn process(&mut self, td:&Telemetry, cfg:&Config, _dt:f32, profile:Option<&CarProfile>) -> Option<ShiftCommand> {
        // FH6 session_timestamp advances on every telemetry packet; only a
        // backwards jump indicates a restarted session/replay clock.
        if let Some(previous) = self.last_timestamp {
            if td.session_timestamp < previous || td.is_race_on == 0 {
                self.last_shift = None;
            }
        }
        self.last_timestamp = Some(td.session_timestamp);
        self.telemetry_lost = false;
        if !cfg.automatic || td.is_race_on==0 || td.gear==0 || td.gear>10 || !self.can_shift(cfg) { return None; }
        if td.engine_max_rpm<=0.0 || td.current_rpm<=0.0 { return None; }

        let speed=td.speed_kmh();
        let throttle=td.throttle();
        let brake=td.brake();
        let pct=td.rpm_pct();

        // Safety guards: avoid shifting while airborne, cornering hard or recovering from oversteer.
        if cfg.features.airtime_lock && td.accel_z.abs() > 1.8 && speed > 35.0 { return None; }
        if cfg.features.cornering_lock && td.accel_x.abs().max(td.accel_y.abs()) > 0.85 { return None; }
        if cfg.features.transient_lock && td.rear_slip() > 0.55 { return None; }

        // Braking / engine-brake strategy. Prefer the learned ratio for the
        // target gear so we estimate post-downshift RPM at the current speed.
        // Never downshift if the predicted RPM would approach the limiter.
        if cfg.brake_downshift && cfg.features.engine_brake && td.gear>1 && speed>15.0
            && throttle<0.18 {
            let brake_threshold = if cfg.features.brake_curve { 0.12 } else { 0.20 };
            if brake >= brake_threshold {
                let target_gear = (td.gear - 1) as usize;
                let predicted_pct = profile.and_then(|p| {
                    let ratio = *p.gear_ratio.get(target_gear)?;
                    let samples = *p.gear_samples.get(target_gear)?;
                    if samples < 8 || !ratio.is_finite() || ratio <= 0.0001 { return None; }
                    let predicted_rpm = td.speed_ms / ratio;
                    let value = predicted_rpm / td.engine_max_rpm;
                    if value.is_finite() { Some(value) } else { None }
                });
                // Stronger braking permits a slightly higher target RPM, but
                // leaves margin below the limiter for telemetry/ratio error.
                let rpm_ceiling = if brake > 0.65 { 0.90 } else if brake > 0.35 { 0.86 } else { 0.82 };
                let rpm_floor = 0.32;
                let safe_to_downshift = match predicted_pct {
                    Some(target_pct) => target_pct >= rpm_floor && target_pct <= rpm_ceiling,
                    // Until this car/gear has enough ratio samples, use a
                    // conservative current-RPM fallback rather than guessing.
                    None => pct < if brake > 0.65 { 0.68 } else { 0.62 },
                };
                let brake_cooldown = cfg.min_shift_interval_ms.min(180);
                let ready_for_brake_shift = self.last_shift
                    .map(|last| last.elapsed().as_millis() >= brake_cooldown as u128)
                    .unwrap_or(true);
                if safe_to_downshift && ready_for_brake_shift {
                    return self.issue(td, td.gear-1, "ENGINE BRAKE");
                }
            }
        }
        // Kickdown: high pedal, engine below power band.
        let kickdown_rpm = if cfg.features.turbo_lag_compensation && td.boost_raw < 0.15 {
            (cfg.kickdown_rpm_pct + 0.04).min(0.75)
        } else { cfg.kickdown_rpm_pct };
        if throttle>=cfg.kickdown_throttle && pct<kickdown_rpm && td.gear>2 && speed>25.0 {
            return self.issue(td, td.gear-1, "KICKDOWN");
        }
        // Recovery when the engine is lugging under load.
        if throttle>0.45 && brake<0.05 && pct<cfg.downshift_rpm_pct && td.gear>2 && speed>20.0 && (!cfg.features.drivetrain_aware || td.rear_slip()<0.45) {
            return self.issue(td, td.gear-1, "TORQUE DOWN");
        }
        // Upshift with hysteresis and a throttle-dependent threshold.
        if throttle>0.12 && brake<0.08 && pct>cfg.upshift_rpm_pct && td.gear<10 {
            return self.issue(td, td.gear+1, "UPSHIFT");
        }
        None
    }
}
