from __future__ import annotations

from dataclasses import dataclass
from time import monotonic

from .calibration import CarProfile
from .models import ShiftDecision, TelemetryFrame, VehicleAnalysis, VehicleState


@dataclass
class ControllerConfig:
    automatic: bool = False
    upshift_rpm_pct: float = 0.91
    redline_guard_pct: float = 0.975
    downshift_rpm_pct: float = 0.58
    braking_downshift_rpm_pct: float = 0.48
    cooldown_s: float = 0.35
    emergency_cooldown_s: float = 0.12
    confirmation_timeout_s: float = 1.4
    failed_shift_retry_s: float = 1.2
    min_speed_kmh: float = 8.0
    max_gear: int = 10
    handbrake_block_pct: float = 0.10
    brake_block_pct: float = 0.15


class ShiftController:
    """State-aware automatic transmission controller with conservative RPM guards."""

    def __init__(self, config: ControllerConfig | None = None) -> None:
        self.config = config or ControllerConfig()
        self._last_shift_at = 0.0
        self._pending_target: int | None = None
        self._pending_since = 0.0
        self._retry_after = 0.0

    def evaluate(self, frame: TelemetryFrame, analysis: VehicleAnalysis,
                 profile: CarProfile | None) -> ShiftDecision:
        c = self.config
        now = monotonic()

        if not frame.race_on:
            self.cancel_pending()
            return ShiftDecision(reason="Race is not active", blocked_by="NO_RACE")
        if frame.gear <= 0:
            return ShiftDecision(reason="Neutral/reverse/unknown gear; automatic shifting inhibited",
                                 blocked_by="INVALID_GEAR")
        if frame.gear > c.max_gear:
            return ShiftDecision(reason="Gear is outside configured range", blocked_by="GEAR_LIMIT")

        handbrake = frame.handbrake_raw / 255.0
        brake = frame.brake
        throttle = frame.throttle
        handbrake_active = handbrake >= c.handbrake_block_pct
        braking_input = brake >= c.brake_block_pct

        # Never issue routine shifts while the handbrake is held. In particular,
        # don't let a wheel-lock-induced RPM spike trigger the redline upshift.
        if handbrake_active:
            return ShiftDecision(reason="Handbrake held: shifting suspended",
                                 blocked_by="HANDBRAKE")

        # Process acknowledgement before making another request.
        if self._pending_target is not None:
            if frame.gear == self._pending_target:
                self._pending_target = None
                self._pending_since = 0.0
            elif now - self._pending_since < c.confirmation_timeout_s:
                return ShiftDecision(reason=f"Waiting for gear {self._pending_target} confirmation",
                                     blocked_by="SHIFT_PENDING")
            else:
                self._pending_target = None
                self._pending_since = 0.0
                self._retry_after = now + c.failed_shift_retry_s
                return ShiftDecision(reason="Requested gear was not confirmed by telemetry",
                                     blocked_by="SHIFT_NOT_CONFIRMED")

        if now < self._retry_after:
            return ShiftDecision(reason="Waiting after an unconfirmed shift",
                                 blocked_by="SHIFT_RETRY_DELAY")

        # Emergency upshift is only valid under positive drive demand. It must
        # never fight braking, handbrake use, a slide, impact, or airborne state.
        safe_for_upshift = (
            not braking_input and throttle >= 0.20
            and analysis.state not in (
                VehicleState.BRAKING, VehicleState.SLIDING, VehicleState.AIRBORNE,
                VehicleState.IMPACT, VehicleState.STOPPED, VehicleState.UNKNOWN,
            )
        )
        if (safe_for_upshift and frame.gear < c.max_gear
                and frame.speed_kmh > 3.0 and frame.engine_max_rpm > 0
                and frame.rpm_pct >= c.redline_guard_pct):
            if now - self._last_shift_at < c.emergency_cooldown_s:
                return ShiftDecision(reason="Emergency upshift cooldown", blocked_by="COOLDOWN")
            target = frame.gear + 1
            ratio = profile.gear_ratio_rpm_per_mps.get(target) if profile else None
            predicted = self._predict_rpm(frame, ratio)
            samples = profile.gear_samples.get(target, 0) if profile else 0
            if predicted is not None and samples >= 30 and predicted >= frame.engine_max_rpm * 1.02:
                return ShiftDecision(reason="Next-gear RPM prediction exceeds safety ceiling",
                                     blocked_by="UNSAFE_PREDICTED_RPM")
            return ShiftDecision(
                requested_gear=target, direction="UP",
                reason="REDLINE SAFETY · engine approaching limiter",
                predicted_rpm=predicted, confidence=0.85 if samples >= 30 else 0.55,
            )

        if frame.speed_kmh < c.min_speed_kmh:
            return ShiftDecision(reason="Vehicle speed too low", blocked_by="LOW_SPEED")

        if analysis.state in (
            VehicleState.STOPPED, VehicleState.SLIDING, VehicleState.AIRBORNE,
            VehicleState.IMPACT, VehicleState.UNKNOWN,
        ):
            return ShiftDecision(reason=f"Shift inhibited in {analysis.state.value}",
                                 blocked_by=analysis.state.value)

        if now - self._last_shift_at < c.cooldown_s:
            return ShiftDecision(reason="Shift cooldown", blocked_by="COOLDOWN")

        up_target = frame.gear + 1
        down_target = frame.gear - 1
        up_ratio = profile.gear_ratio_rpm_per_mps.get(up_target) if profile else None
        down_ratio = profile.gear_ratio_rpm_per_mps.get(down_target) if profile else None
        down_samples = profile.gear_samples.get(down_target, 0) if profile else 0

        # Braking downshift: only one gear at a time, only when engine speed has
        # fallen enough, and only if the learned target ratio predicts safe RPM.
        if (braking_input and analysis.state == VehicleState.BRAKING
                and frame.gear > 1 and frame.rpm_pct <= c.braking_downshift_rpm_pct
                and down_ratio is not None and down_samples >= 30):
            predicted = self._predict_rpm(frame, down_ratio)
            if (predicted is not None
                    and frame.idle_rpm * 1.20 < predicted < frame.engine_max_rpm * 0.86):
                return self._decision(down_target, "BRAKING DOWNSHIFT · safe engine RPM",
                                      predicted, 0.80)

        # Kickdown: react to strong throttle demand before RPM becomes excessively
        # low. Without a learned ratio, hold rather than guess a gear.
        if (not braking_input and throttle >= 0.78
                and frame.rpm_pct <= c.downshift_rpm_pct
                and frame.gear > 1
                and analysis.state in (VehicleState.ACCELERATING, VehicleState.CRUISING)
                and profile is not None and down_ratio is not None and down_samples >= 30):
            predicted = self._predict_rpm(frame, down_ratio)
            if (predicted is not None
                    and frame.idle_rpm * 1.20 < predicted < frame.engine_max_rpm * 0.88):
                return self._decision(down_target, "KICKDOWN · high throttle, low RPM",
                                      predicted, 0.78)

        # Normal upshift: require actual throttle and measured acceleration;
        # neither braking nor handbrake input may trigger it.
        if (not braking_input and throttle >= 0.72
                and analysis.state == VehicleState.ACCELERATING
                and frame.rpm_pct >= c.upshift_rpm_pct
                and up_target <= c.max_gear):
            if up_ratio is not None and profile and profile.gear_samples.get(up_target, 0) >= 30:
                predicted = self._predict_rpm(frame, up_ratio)
                if predicted is not None and frame.idle_rpm * 1.2 < predicted < frame.engine_max_rpm * 0.94:
                    current_power = profile.estimated_power_kw(frame.rpm_pct)
                    next_power = profile.estimated_power_kw(predicted / frame.engine_max_rpm)
                    if current_power is None or next_power is None or next_power >= current_power * 0.98:
                        return self._decision(up_target, "LEARNED UPSHIFT MODEL", predicted, 0.78)
                else:
                    return ShiftDecision(reason="Next gear RPM prediction is outside safe range",
                                         blocked_by="UNSAFE_PREDICTED_RPM")
            elif up_target <= c.max_gear:
                # Conservative single-step fallback is still permitted, but only
                # when the car is genuinely accelerating under throttle.
                return ShiftDecision(
                    requested_gear=up_target, direction="UP",
                    reason="RPM THRESHOLD FALLBACK · per-car ratio still learning",
                    confidence=0.40,
                )

        return ShiftDecision(reason="Current gear remains appropriate")

    @staticmethod
    def _predict_rpm(frame: TelemetryFrame, ratio: float | None) -> float | None:
        if ratio is None or not (1.0 < ratio < 2000.0) or frame.speed_mps <= 0:
            return None
        predicted = frame.speed_mps * ratio
        return predicted if predicted > 0 else None

    def _decision(self, target: int, reason: str, predicted: float,
                  confidence: float) -> ShiftDecision:
        direction = "UP" if target > 0 and reason.startswith(("REDLINE", "LEARNED UPSHIFT")) else "DOWN"
        return ShiftDecision(requested_gear=target, direction=direction,
                             reason=reason, predicted_rpm=predicted, confidence=confidence)

    def confirm_sent(self, target_gear: int) -> None:
        self._pending_target = target_gear
        self._pending_since = monotonic()
        self._last_shift_at = self._pending_since

    def cancel_pending(self) -> None:
        self._pending_target = None
        self._pending_since = 0.0
