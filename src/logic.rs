use crate::{config::Config, input::ShiftCommand, telemetry::Telemetry};
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
    pub fn process(&mut self, td:&Telemetry, cfg:&Config, _dt:f32) -> Option<ShiftCommand> {
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

        // Braking downshift: one gear per decision, with a rev ceiling.
        let brake_threshold = if cfg.features.brake_curve { 0.20 } else { 0.25 };
        if cfg.brake_downshift && brake>brake_threshold && td.gear>1 && speed>25.0 && pct<0.76 {
            return self.issue(td, td.gear-1, "BRAKE DOWN");
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
