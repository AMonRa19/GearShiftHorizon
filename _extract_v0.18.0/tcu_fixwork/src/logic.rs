use crate::{calibration::CarProfile, config::Config, input::ShiftCommand, telemetry::Telemetry};
use std::time::{Duration, Instant};

// Match the upstream Virtual TCU shift cooldown (Cfg.SHIFT_COOLDOWN_MS).
const SHIFT_COOLDOWN_MS: u64 = 350;

#[derive(Default)]
pub struct Tcu {
    last_shift: Option<Instant>,
    last_timestamp: Option<u32>,
    telemetry_lost: bool,
    pending_shift: Option<(u8, u8, Instant)>,
    last_shift_was_braking: bool,
    upshift_armed: bool,
    last_speed_kmh: Option<f32>,
    last_speed_timestamp: Option<u32>,
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
        if samples < 12 || !ratio.is_finite() || ratio <= 1.0 || ratio >= 2000.0 { return None; }
        // Calibration stores RPM per m/s (not its inverse).
        let predicted_rpm = td.speed_ms * ratio;
        let pct = predicted_rpm / td.engine_max_rpm;
        if pct.is_finite() && pct > 0.0 { Some(pct) } else { None }
    }

    pub fn process(&mut self, td: &Telemetry, cfg: &Config, _dt: f32, profile: Option<&CarProfile>) -> Option<ShiftCommand> {
        if let Some(previous) = self.last_timestamp {
            if td.session_timestamp == previous { return None; }
            if td.session_timestamp < previous || td.is_race_on == 0 {
                self.last_shift = None;
                self.pending_shift = None;
                self.last_shift_was_braking = false;
                self.upshift_armed = true;
                self.last_speed_kmh = None;
                self.last_speed_timestamp = None;
            }
        }
        self.last_timestamp = Some(td.session_timestamp);
        self.telemetry_lost = false;

        // Confirm a requested shift before issuing another one. An unexpected
        // gear change cancels the pending command too, but never triggers a cascade.
        if let Some((from, target, sent_at)) = self.pending_shift {
            if td.gear == target {
                self.pending_shift = None;
            } else if sent_at.elapsed() >= Duration::from_millis(1400) {
                self.pending_shift = None;
            } else {
                // A multi-gear command is emitted as a sequence of key pulses.
                // Intermediate gear reports are progress, not failed shifts.
                let moving_toward_target = if target > from {
                    td.gear > from && td.gear < target
                } else {
                    td.gear < from && td.gear > target
                };
                if moving_toward_target { return None; }
                if td.gear != from {
                    self.pending_shift = None;
                    self.last_shift = Some(Instant::now());
                } else {
                    return None;
                }
            }
        }

        if self.telemetry_lost || !cfg.automatic || td.is_race_on == 0 || td.gear == 0 || td.gear > 10
            || td.engine_max_rpm <= 0.0 || td.current_rpm <= 0.0 {
            return None;
        }

        let speed = td.speed_kmh();
        let speed_delta = self.last_speed_kmh.map(|previous| previous - speed);
        let speed_dt = self.last_speed_timestamp.map(|previous| td.session_timestamp.wrapping_sub(previous) as f32 / 1000.0);
        let decel_ms2 = match (speed_delta, speed_dt) {
            (Some(drop_kmh), Some(dt)) if dt > 0.005 && dt <= 0.20 => (drop_kmh / 3.6 / dt).clamp(-30.0, 30.0),
            _ => 0.0,
        };
        let speed_falling = speed_delta.map(|drop| drop > 0.10).unwrap_or(false);
        let sudden_speed_loss = decel_ms2 > 2.2 || speed_delta.map(|drop| drop > 7.0).unwrap_or(false);
        self.last_speed_kmh = Some(speed);
        self.last_speed_timestamp = Some(td.session_timestamp);
        let throttle = td.throttle();
        let brake = td.brake();
        let pct = td.rpm_pct();

        // Hard redline protection: do not let corner/traction locks or a normal
        // shift threshold keep the engine on the limiter. Pending acknowledgement
        // still prevents repeated key pulses for the same requested shift.
        if pct >= 0.985 && td.gear < 10 && speed > 3.0
            && self.elapsed_since_shift() >= Duration::from_millis(120) {
            return self.issue(td, td.gear + 1, "REDLINE OVERRIDE");
        }

        // Profile presets are applied as runtime overlays; the user's base settings
        // remain intact when switching between Comfort/Race/Drift/Drag/Off-road.
        let (upshift_pct, downshift_pct, kickdown_throttle, kickdown_rpm, shift_delay_ms,
             brake_delay_ms, corner_limit, slip_limit, engine_brake_enabled) = match cfg.mode.as_str() {
            "Drag" => (
                cfg.upshift_rpm_pct.max(0.975).min(0.985),
                cfg.downshift_rpm_pct.min(0.32),
                cfg.kickdown_throttle.max(0.88),
                cfg.kickdown_rpm_pct.min(0.48),
                SHIFT_COOLDOWN_MS,
                SHIFT_COOLDOWN_MS,
                0.95, 0.60, true,
            ),
            "Drift" => (
                cfg.upshift_rpm_pct.max(0.955).min(0.99),
                cfg.downshift_rpm_pct.max(0.40).min(0.62),
                cfg.kickdown_throttle.min(0.60),
                cfg.kickdown_rpm_pct.max(0.60).min(0.76),
                SHIFT_COOLDOWN_MS,
                SHIFT_COOLDOWN_MS,
                1.15, 0.88, false,
            ),
            "Race" => (
                cfg.upshift_rpm_pct.max(0.91).min(0.985),
                cfg.downshift_rpm_pct.max(0.28).min(0.58),
                cfg.kickdown_throttle,
                cfg.kickdown_rpm_pct,
                SHIFT_COOLDOWN_MS,
                SHIFT_COOLDOWN_MS,
                0.95, 0.68, true,
            ),
            "Off-road" => (
                cfg.upshift_rpm_pct.min(0.90),
                cfg.downshift_rpm_pct.max(0.34).min(0.58),
                cfg.kickdown_throttle.max(0.72),
                cfg.kickdown_rpm_pct,
                SHIFT_COOLDOWN_MS,
                SHIFT_COOLDOWN_MS,
                1.05, 0.78, true,
            ),
            _ => (
                cfg.upshift_rpm_pct,
                cfg.downshift_rpm_pct,
                cfg.kickdown_throttle,
                cfg.kickdown_rpm_pct,
                SHIFT_COOLDOWN_MS,
                SHIFT_COOLDOWN_MS,
                0.85, 0.55, true,
            ),
        };
        if self.last_shift.is_none() || pct < (upshift_pct - 0.08).max(0.50) || throttle < 0.08 { self.upshift_armed = true; }

        // Keep the current gear while airborne or during high lateral load.
        // This allows braking downshifts on approach, but avoids unsettling the car mid-corner.
        // In Forza telemetry accel_y is strongly affected by longitudinal
        // braking/acceleration. Only lateral acceleration should inhibit a
        // shift; otherwise hard braking can accidentally lock out downshifts.
        let lateral_g = td.accel_x.abs();
        let in_high_lateral_load = lateral_g > corner_limit;
        let braking_intent = brake >= if cfg.features.brake_curve { 0.08 } else { 0.13 }
            && throttle < 0.25;
        // Airtime lock suppresses routine shifts, but must not prevent a
        // braking recovery or a post-impact downshift.
        if cfg.features.airtime_lock && td.accel_z.abs() > 1.8 && speed > 35.0
            && !braking_intent && !speed_falling && !sudden_speed_loss { return None; }
        // Cornering lock is for torque-driven shifts in a loaded turn. It must
        // never suppress a deliberate braking downshift on corner entry.
        if cfg.features.cornering_lock && in_high_lateral_load && !braking_intent && !speed_falling && !sudden_speed_loss { return None; }
        // Traction locks protect acceleration shifts, but must not deadlock
        // recovery downshifts during braking or after a sudden speed loss.
        if cfg.features.transient_lock && td.rear_slip() > slip_limit && !braking_intent && !speed_falling && !sudden_speed_loss { return None; }

        // Engine braking: react to brake pedal before the car nearly stops.
        // A lower gear is allowed when predicted RPM remains below a safety
        // ceiling. If the next gear would over-rev, wait for road speed to fall
        // and re-evaluate on the next telemetry packets.
        if cfg.brake_downshift && cfg.features.engine_brake && engine_brake_enabled
            && td.gear > 1 && speed > 4.0 && throttle < 0.25 && braking_intent {
            let target_gear = profile.and_then(|p| p.best_lower_gear(td.speed_ms, td.engine_max_rpm, td.gear)).unwrap_or(td.gear - 1);
            let learned = Self::predicted_target_rpm_pct(td, profile, target_gear as usize);
            // Conservative fallback until this car/gear has enough clean samples.
            let target_pct = learned.unwrap_or(pct * (1.28 + (td.gear.saturating_sub(2) as f32 * 0.012)));
            let rpm_ceiling = if brake > 0.75 { 0.94 } else if brake > 0.35 { 0.90 } else { 0.86 };
            // Do not wait for a high RPM threshold: at high gears the engine
            // can be well below 30% RPM while the next gear is still useful.
            // Keep only a small margin above idle to avoid an excessively low
            // target on slow approaches.
            let idle_floor = if learned.is_some() {
                (td.idle_rpm / td.engine_max_rpm + 0.025).clamp(0.10, 0.25)
            } else { 0.08 };
            let safe_to_downshift = target_pct.is_finite()
                && target_pct >= idle_floor && target_pct <= rpm_ceiling;
            // On the approach to a corner, allow braking downshifts even under
            // longitudinal deceleration; only sustained high lateral G blocks them.
            if safe_to_downshift && (!in_high_lateral_load || brake >= 0.65)
                && self.elapsed_since_shift() >= Duration::from_millis(brake_delay_ms) {
                return self.issue(td, target_gear, "ENGINE BRAKE");
            }
        }

        // Deceleration recovery: when speed is falling while the engine is lugging,
        // downshift progressively instead of waiting for the car to nearly stop.
        // This path also covers collisions and coasting after a sudden speed loss.
        if td.gear > 1 && speed > 3.0 && (speed_falling || sudden_speed_loss) && (throttle < 0.45 || sudden_speed_loss)
            && self.elapsed_since_shift() >= Duration::from_millis(SHIFT_COOLDOWN_MS) {
            let learned_target = profile.and_then(|p| p.best_lower_gear(td.speed_ms, td.engine_max_rpm, td.gear));
            let target_gear = learned_target.unwrap_or(td.gear - 1);
            let next_pct = Self::predicted_target_rpm_pct(td, profile, target_gear as usize)
                .unwrap_or(pct * (1.28 + (td.gear.saturating_sub(target_gear + 1) as f32 * 0.025)));
            if next_pct.is_finite() && next_pct >= 0.08 && next_pct <= 0.88 {
                return self.issue(td, target_gear, if sudden_speed_loss { "IMPACT RECOVERY" } else { "DECELERATION RECOVERY" });
            }
        }

        // Wrong-gear launch recovery: if the car is moving slowly in a tall
        // gear under meaningful throttle, select a learned lower gear when
        // available; otherwise downshift one step conservatively.
        if td.gear >= 3 && speed > 2.0 && speed < 32.0 && throttle > 0.48 && brake < 0.05
            && pct < 0.58 && self.elapsed_since_shift() >= Duration::from_millis(SHIFT_COOLDOWN_MS) {
            let learned_target = profile.and_then(|p| p.best_lower_gear(td.speed_ms, td.engine_max_rpm, td.gear));
            let target_gear = learned_target.unwrap_or(td.gear - 1);
            let target_pct = Self::predicted_target_rpm_pct(td, profile, target_gear as usize)
                .unwrap_or(pct * 1.24);
            if target_pct.is_finite() && target_pct >= 0.12 && target_pct <= 0.88 {
                return self.issue(td, target_gear, "WRONG GEAR LAUNCH RECOVERY");
            }
        }

        // Normal torque downshifts are slower than braking shifts to avoid hunting.
        let normal_interval = shift_delay_ms;
        let effective_kickdown_rpm = if cfg.features.turbo_lag_compensation && td.boost_raw < 0.15 {
            (kickdown_rpm + 0.04).min(0.80)
        } else { kickdown_rpm };
        let style_aggression = if cfg.features.drive_style_adaptive {
            profile.map(|p| p.drive_style_aggression).unwrap_or(0.5).clamp(0.0, 1.0)
        } else { 0.5 };
        // Aggressive driving requests kickdown earlier; gentle cruising keeps
        // a taller gear longer. The adjustment is deliberately bounded.
        let adaptive_kickdown_throttle = (kickdown_throttle - (style_aggression - 0.5) * 0.16).clamp(0.52, 0.94);
        let next_gear_rpm = Self::predicted_target_rpm_pct(td, profile, (td.gear - 1) as usize)
            .unwrap_or(pct * 1.24);
        let torque_downshift_safe = next_gear_rpm.is_finite() && next_gear_rpm <= 0.92;
        if self.elapsed_since_shift() >= Duration::from_millis(normal_interval.min(360))
            && throttle >= adaptive_kickdown_throttle && pct < effective_kickdown_rpm.max(0.62)
            && td.gear > 1 && speed > 2.0 && torque_downshift_safe {
            let target = profile.and_then(|p| p.best_lower_gear(td.speed_ms, td.engine_max_rpm, td.gear)).unwrap_or(td.gear - 1);
            let safe_pct = Self::predicted_target_rpm_pct(td, profile, target as usize).unwrap_or(next_gear_rpm);
            if target < td.gear && safe_pct <= 0.92 { return self.issue(td, target, "KICKDOWN"); }
        }
        if self.elapsed_since_shift() >= Duration::from_millis(normal_interval.min(420))
            && throttle > 0.52 && brake < 0.05 && pct < downshift_pct.max(0.46)
            && td.gear > 1 && speed > 2.0 && torque_downshift_safe
            && (!cfg.features.drivetrain_aware || td.rear_slip() < 0.60) {
            let target = profile.and_then(|p| p.best_lower_gear(td.speed_ms, td.engine_max_rpm, td.gear)).unwrap_or(td.gear - 1);
            let safe_pct = Self::predicted_target_rpm_pct(td, profile, target as usize).unwrap_or(next_gear_rpm);
            if target < td.gear && safe_pct <= 0.92 { return self.issue(td, target, "TORQUE DOWN"); }
        }

        // The original 350 ms cooldown is shared by upshifts and downshifts.
        // The pending-shift acknowledgement and upshift_armed latch prevent cascades.
        let upshift_interval = shift_delay_ms;
        let recently_brake_shifted = self.last_shift_was_braking && self.elapsed_since_shift() < Duration::from_millis(600);
        if self.upshift_armed && !recently_brake_shifted && self.elapsed_since_shift() >= Duration::from_millis(upshift_interval)
            && throttle > 0.18 && brake < 0.05 && pct > upshift_pct && td.gear < 10 {
            return self.issue(td, td.gear + 1, "UPSHIFT");
        }
        None
    }
}
