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

    fn predicted_target_rpm_pct_at_speed(td: &Telemetry, profile: Option<&CarProfile>, target_gear: usize, speed_ms: f32) -> Option<f32> {
        let p = profile?;
        let ratio = *p.gear_ratio.get(target_gear)?;
        let samples = *p.gear_samples.get(target_gear)?;
        if samples < 3 || !ratio.is_finite() || ratio <= 1.0 || ratio >= 2000.0 || td.engine_max_rpm <= 0.0 { return None; }
        // Calibration stores RPM per m/s (not its inverse).
        let predicted_rpm = speed_ms.max(0.0) * ratio;
        let pct = predicted_rpm / td.engine_max_rpm;
        if pct.is_finite() && pct > 0.0 { Some(pct) } else { None }
    }

    fn predicted_target_rpm_pct(td: &Telemetry, profile: Option<&CarProfile>, target_gear: usize) -> Option<f32> {
        Self::predicted_target_rpm_pct_at_speed(td, profile, target_gear, td.speed_ms)
    }

    // Pick the lowest useful gear at the projected speed, then clamp it against
    // the RPM that the engine would see at the car's actual speed.
    fn braking_target_gear(td: &Telemetry, profile: Option<&CarProfile>, brake: f32) -> Option<u8> {
        let p = profile?;
        let projected_speed = td.speed_ms * (1.0 - 0.20 * (brake / 0.80).clamp(0.0, 1.0));
        let idle_floor = (td.idle_rpm / td.engine_max_rpm + 0.035).clamp(0.16, 0.32);
        let projected_ceiling = if brake > 0.75 { 0.90 } else { 0.84 };
        let actual_ceiling = if brake > 0.75 { 0.94 } else if brake > 0.35 { 0.90 } else { 0.86 };
        let mut desired = None;
        for gear in 1..td.gear {
            if let Some(rpm) = Self::predicted_target_rpm_pct_at_speed(td, Some(p), gear as usize, projected_speed) {
                if rpm >= idle_floor && rpm <= projected_ceiling { desired = Some(gear); break; }
            }
        }
        let mut target = desired?;
        while target < td.gear {
            match Self::predicted_target_rpm_pct(td, Some(p), target as usize) {
                Some(rpm) if rpm <= actual_ceiling => return Some(target),
                Some(_) => target += 1,
                None => return None,
            }
        }
        None
    }

    // Select the lower gear whose learned RPM lands closest to the engine's
    // strongest part of its measured power curve. Never select a gear that
    // would immediately trigger an upshift (anti-hunting).
    fn power_target_gear(td: &Telemetry, profile: Option<&CarProfile>, upshift_pct: f32) -> Option<u8> {
        let p = profile?;
        let target_rpm = p.power_curve.iter().filter(|x| x.samples >= 3 && x.rpm_pct.is_finite() && x.power_kw.is_finite())
            .max_by(|a,b| a.power_kw.total_cmp(&b.power_kw)).map(|x| x.rpm_pct).unwrap_or(0.76);
        let mut best: Option<(u8, f32)> = None;
        for gear in 1..td.gear {
            if let Some(rpm) = Self::predicted_target_rpm_pct(td, Some(p), gear as usize) {
                if rpm < 0.50 || rpm > 0.92 || rpm >= upshift_pct - 0.055 { continue; }
                let score = (rpm - target_rpm).abs();
                if best.map(|(_, old)| score < old).unwrap_or(true) { best = Some((gear, score)); }
            }
        }
        best.map(|(gear, _)| gear)
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

        // Confirm the final requested gear before issuing another command.
        // Intermediate gears are expected during a multi-step downshift/upshift.
        if let Some((from, target, sent_at)) = self.pending_shift {
            if td.gear == target {
                self.pending_shift = None;
            } else if td.gear != from && ((target < from && td.gear < from && td.gear > target)
                || (target > from && td.gear > from && td.gear < target)) {
                // A multi-step keyboard command passes through intermediate gears.
                // Keep it pending until the final requested gear is observed.
                if sent_at.elapsed() >= Duration::from_millis(1400) { self.pending_shift = None; }
                else { return None; }
            } else if td.gear != from {
                self.pending_shift = None;
                self.last_shift = Some(Instant::now());
            } else if sent_at.elapsed() >= Duration::from_millis(1400) {
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
        // In Forza telemetry accel_y is strongly affected by longitudinal
        // braking/acceleration. Only lateral acceleration should inhibit a
        // shift; otherwise hard braking can accidentally lock out downshifts.
        let lateral_g = td.accel_x.abs();
        let in_high_lateral_load = lateral_g > corner_limit;
        let braking_intent = brake >= if cfg.features.brake_curve { 0.08 } else { 0.13 }
            && throttle < 0.25;
        // Cornering lock is for torque-driven shifts in a loaded turn. It must
        // never suppress a deliberate braking downshift on corner entry.
        if cfg.features.cornering_lock && in_high_lateral_load && !braking_intent { return None; }
        if cfg.features.transient_lock && td.rear_slip() > slip_limit { return None; }

        // Engine braking: react to brake pedal before the car nearly stops.
        // A lower gear is allowed when predicted RPM remains below a safety
        // ceiling. If the next gear would over-rev, wait for road speed to fall
        // and re-evaluate on the next telemetry packets.
        if cfg.brake_downshift && cfg.features.engine_brake && engine_brake_enabled
            && td.gear > 1 && speed > 8.0 && throttle < 0.25 && braking_intent {
            let target = Self::braking_target_gear(td, profile, brake).unwrap_or_else(|| {
                // Until enough per-gear samples exist, retain the conservative
                // one-step behavior; never extrapolate several gears from pct*1.24.
                let next_pct = Self::predicted_target_rpm_pct(td, profile, (td.gear - 1) as usize)
                    .unwrap_or(pct * 1.24);
                if next_pct.is_finite() && next_pct >= 0.20 && next_pct <= 0.86 { td.gear - 1 } else { td.gear }
            });
            if target < td.gear && (!in_high_lateral_load || brake >= 0.65)
                && self.elapsed_since_shift() >= Duration::from_millis(brake_delay_ms.min(260)) {
                return self.issue(td, target, "ENGINE BRAKE");
            }
        }

        // Normal torque downshifts are slower than braking shifts to avoid hunting.
        let normal_interval = shift_delay_ms;
        let effective_kickdown_rpm = if cfg.features.turbo_lag_compensation && td.boost_raw < 0.15 {
            (kickdown_rpm + 0.04).min(0.80)
        } else { kickdown_rpm };
        let power_target = Self::power_target_gear(td, profile, upshift_pct);
        let next_gear_rpm = Self::predicted_target_rpm_pct(td, profile, (td.gear - 1) as usize)
            .unwrap_or(pct * 1.24);
        let torque_downshift_safe = next_gear_rpm.is_finite() && next_gear_rpm <= 0.92;
        if self.elapsed_since_shift() >= Duration::from_millis(normal_interval.min(360))
            && throttle >= kickdown_throttle && pct < effective_kickdown_rpm.max(0.62)
            && td.gear > 1 && speed > 18.0 {
            if let Some(target) = power_target.filter(|g| *g < td.gear) {
                return self.issue(td, target, "KICKDOWN");
            }
            if torque_downshift_safe { return self.issue(td, td.gear - 1, "KICKDOWN"); }
        }
        if self.elapsed_since_shift() >= Duration::from_millis(normal_interval.min(420))
            && throttle > 0.52 && brake < 0.05 && pct < downshift_pct.max(0.46)
            && td.gear > 1 && speed > 18.0 && torque_downshift_safe
            && (!cfg.features.drivetrain_aware || td.rear_slip() < 0.60) {
            let target = power_target.filter(|g| *g < td.gear).unwrap_or(td.gear - 1);
            return self.issue(td, target, "TORQUE DOWN");
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
