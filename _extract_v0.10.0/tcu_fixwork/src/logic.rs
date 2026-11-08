use crate::{calibration::CarProfile, config::Config, input::ShiftCommand, telemetry::Telemetry};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Tcu {
    last_shift: Option<Instant>,
    last_timestamp: Option<u32>,
    telemetry_lost: bool,
    pending_shift: Option<(u8, u8, Instant)>,
    last_shift_was_braking: bool,
    upshift_armed: bool,
}
impl Tcu {
    pub fn on_telemetry_timeout(&mut self) { self.telemetry_lost = true; }

    fn elapsed_since_shift(&self) -> Duration {
        self.last_shift.map(|t| t.elapsed()).unwrap_or(Duration::MAX)
    }

    fn issue(&mut self, td: &Telemetry, target: u8, reason: &'static str) -> Option<ShiftCommand> {
        if target == td.gear { return None; }
        let now = Instant::now();
        self.last_shift = Some(now);
        self.last_shift_was_braking = reason == "ENGINE BRAKE";
        if reason == "UPSHIFT" { self.upshift_armed = false; }
        self.pending_shift = Some((td.gear, target, now));
        Some(ShiftCommand { from: td.gear, to: target, reason })
    }

    fn predicted_target_rpm_pct(td: &Telemetry, profile: Option<&CarProfile>, target_gear: usize) -> Option<f32> {
        let p = profile?;
        let ratio = *p.gear_ratio.get(target_gear)?;
        let samples = *p.gear_samples.get(target_gear)?;
        if samples < 3 || !ratio.is_finite() || ratio <= 0.0001 { return None; }
        let predicted_rpm = td.speed_ms / ratio;
        let pct = predicted_rpm / td.engine_max_rpm;
        if pct.is_finite() && pct > 0.0 { Some(pct) } else { None }
    }

    pub fn process(&mut self, td: &Telemetry, cfg: &Config, _dt: f32, profile: Option<&CarProfile>) -> Option<ShiftCommand> {
        if let Some(previous) = self.last_timestamp {
            if td.session_timestamp < previous || td.is_race_on == 0 {
                self.last_shift = None;
                self.pending_shift = None;
                self.last_shift_was_braking = false;
                self.upshift_armed = true;
            }
        }
        self.last_timestamp = Some(td.session_timestamp);
        self.telemetry_lost = false;

        // Confirm a requested shift before issuing another one. An unexpected
        // gear change cancels the pending command too, but never triggers a cascade.
        if let Some((from, target, sent_at)) = self.pending_shift {
            if td.gear == target {
                self.pending_shift = None;
            } else if td.gear != from {
                self.pending_shift = None;
                self.last_shift = Some(Instant::now());
            } else if sent_at.elapsed() >= Duration::from_millis(850) {
                self.pending_shift = None;
            } else {
                return None;
            }
        }

        if self.telemetry_lost || !cfg.automatic || td.is_race_on == 0 || td.gear == 0 || td.gear > 10
            || td.engine_max_rpm <= 0.0 || td.current_rpm <= 0.0 {
            return None;
        }

        let speed = td.speed_kmh();
        let throttle = td.throttle();
        let brake = td.brake();
        let pct = td.rpm_pct();

        // Profile presets are applied as runtime overlays; the user's base settings
        // remain intact when switching between Comfort/Race/Drift/Drag/Off-road.
        let (upshift_pct, downshift_pct, kickdown_throttle, kickdown_rpm, shift_delay_ms,
             brake_delay_ms, corner_limit, slip_limit, engine_brake_enabled) = match cfg.mode.as_str() {
            "Drag" => (
                cfg.upshift_rpm_pct.max(0.975).min(0.985),
                cfg.downshift_rpm_pct.min(0.32),
                cfg.kickdown_throttle.max(0.88),
                cfg.kickdown_rpm_pct.min(0.48),
                cfg.shift_delay_ms.clamp(280, 650),
                cfg.shift_delay_ms.clamp(220, 500),
                0.95, 0.60, true,
            ),
            "Drift" => (
                cfg.upshift_rpm_pct.max(0.955).min(0.99),
                cfg.downshift_rpm_pct.max(0.40).min(0.62),
                cfg.kickdown_throttle.min(0.60),
                cfg.kickdown_rpm_pct.max(0.60).min(0.76),
                cfg.shift_delay_ms.clamp(120, 260),
                cfg.shift_delay_ms.clamp(180, 320),
                1.15, 0.88, false,
            ),
            "Race" => (
                cfg.upshift_rpm_pct.max(0.91).min(0.985),
                cfg.downshift_rpm_pct.max(0.28).min(0.58),
                cfg.kickdown_throttle,
                cfg.kickdown_rpm_pct,
                cfg.shift_delay_ms.clamp(180, 420),
                cfg.shift_delay_ms.clamp(180, 360),
                0.95, 0.68, true,
            ),
            "Off-road" => (
                cfg.upshift_rpm_pct.min(0.90),
                cfg.downshift_rpm_pct.max(0.34).min(0.58),
                cfg.kickdown_throttle.max(0.72),
                cfg.kickdown_rpm_pct,
                cfg.shift_delay_ms.clamp(300, 700),
                cfg.shift_delay_ms.clamp(260, 520),
                1.05, 0.78, true,
            ),
            _ => (
                cfg.upshift_rpm_pct,
                cfg.downshift_rpm_pct,
                cfg.kickdown_throttle,
                cfg.kickdown_rpm_pct,
                cfg.shift_delay_ms.clamp(250, 900),
                cfg.shift_delay_ms.clamp(220, 650),
                0.85, 0.55, true,
            ),
        };
        if self.last_shift.is_none() || pct < (upshift_pct - 0.08).max(0.50) || throttle < 0.08 { self.upshift_armed = true; }

        // Keep the current gear while airborne or during high lateral load.
        // This allows braking downshifts on approach, but avoids unsettling the car mid-corner.
        if cfg.features.airtime_lock && td.accel_z.abs() > 1.8 && speed > 35.0 { return None; }
        if cfg.features.cornering_lock && td.accel_x.abs().max(td.accel_y.abs()) > corner_limit { return None; }
        if cfg.features.transient_lock && td.rear_slip() > slip_limit { return None; }

        // Engine braking: choose a lower gear only when its predicted RPM is in
        // a useful braking band and safely below the limiter. With sparse learning
        // data, estimate a conservative one-step ratio instead of waiting forever.
        if cfg.brake_downshift && cfg.features.engine_brake && engine_brake_enabled && td.gear > 1 && speed > 12.0
            && throttle < 0.18 && brake >= if cfg.features.brake_curve { 0.10 } else { 0.16 } {
            let target_gear = (td.gear - 1) as usize;
            let learned = Self::predicted_target_rpm_pct(td, profile, target_gear);
            let estimated = learned.or_else(|| {
                // Typical adjacent-gear RPM rise is roughly 18–30%; deliberately
                // use the lower end until this exact car/gear has been learned.
                let value = pct * 1.20;
                if value.is_finite() { Some(value) } else { None }
            });
            let target_pct = estimated.unwrap_or(1.0);
            let rpm_ceiling = if brake > 0.70 { 0.88 } else if brake > 0.35 { 0.84 } else { 0.80 };
            let useful_rpm = target_pct >= 0.42 && target_pct <= rpm_ceiling;
            // Faster sequential braking shifts, but still wait for telemetry confirmation.
            let brake_interval = brake_delay_ms;
            if useful_rpm && self.elapsed_since_shift() >= Duration::from_millis(brake_interval) {
                return self.issue(td, td.gear - 1, "ENGINE BRAKE");
            }
        }

        // Normal torque downshifts are slower than braking shifts to avoid hunting.
        let normal_interval = shift_delay_ms;
        let effective_kickdown_rpm = if cfg.features.turbo_lag_compensation && td.boost_raw < 0.15 {
            (kickdown_rpm + 0.04).min(0.80)
        } else { kickdown_rpm };
        if self.elapsed_since_shift() >= Duration::from_millis(normal_interval)
            && throttle >= kickdown_throttle && pct < effective_kickdown_rpm && td.gear > 2 && speed > 25.0 {
            return self.issue(td, td.gear - 1, "KICKDOWN");
        }
        if self.elapsed_since_shift() >= Duration::from_millis(normal_interval)
            && throttle > 0.45 && brake < 0.05 && pct < downshift_pct
            && td.gear > 2 && speed > 20.0
            && (!cfg.features.drivetrain_aware || td.rear_slip() < 0.45) {
            return self.issue(td, td.gear - 1, "TORQUE DOWN");
        }

        // Stronger anti-cascade protection on upshifts prevents 7 -> 8 -> 9 -> 10
        // in rapid succession while RPM remains above the threshold after each shift.
        let upshift_interval = shift_delay_ms.max(900);
        let recently_brake_shifted = self.last_shift_was_braking && self.elapsed_since_shift() < Duration::from_millis(650);
        if self.upshift_armed && !recently_brake_shifted && self.elapsed_since_shift() >= Duration::from_millis(upshift_interval)
            && throttle > 0.18 && brake < 0.05 && pct > upshift_pct && td.gear < 10 {
            return self.issue(td, td.gear + 1, "UPSHIFT");
        }
        None
    }
}
