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
    downshift_rpm_pct: float = 0.34
    cooldown_s: float = 0.35
    emergency_cooldown_s: float = 0.12
    confirmation_timeout_s: float = 1.4
    failed_shift_retry_s: float = 1.2
    min_speed_kmh: float = 8.0
    max_gear: int = 10


class ShiftController:
    """Conservative prediction-based controller. Missing data means HOLD, never guess."""

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

        # Critical redline guard runs before routine driving-state locks and pending
        # downshift acknowledgement, matching the intended emergency priority.
        if (frame.gear < c.max_gear and frame.speed_kmh > 3.0
                and frame.engine_max_rpm > 0
                and frame.rpm_pct >= c.redline_guard_pct):
            emergency_target = frame.gear + 1
            if self._pending_target == emergency_target:
                return ShiftDecision(reason="Emergency upshift already pending",
                                     blocked_by="SHIFT_PENDING")
            if now - self._last_shift_at < c.emergency_cooldown_s:
                return ShiftDecision(reason="Emergency upshift cooldown",
                                     blocked_by="COOLDOWN")
            self._pending_target = None
            self._pending_since = 0.0
            ratio = profile.gear_ratio_rpm_per_mps.get(emergency_target) if profile else None
            predicted = self._predict_rpm(frame, ratio)
            samples = profile.gear_samples.get(emergency_target, 0) if profile else 0
            if predicted is not None and samples >= 30 and predicted >= frame.engine_max_rpm * 1.02:
                return ShiftDecision(reason="Next-gear RPM prediction exceeds safety ceiling",
                                     blocked_by="UNSAFE_PREDICTED_RPM")
            return ShiftDecision(requested_gear=emergency_target, direction="UP",
                                 reason=("REDLINE SAFETY" if predicted is not None and samples >= 30
                                         else "REDLINE SAFETY · adjacent ratio not yet learned"),
                                 predicted_rpm=predicted, confidence=0.8 if samples >= 30 else 0.55)

        if frame.speed_kmh < c.min_speed_kmh:
            return ShiftDecision(reason="Vehicle speed too low", blocked_by="LOW_SPEED")
        if analysis.state in (VehicleState.STOPPED, VehicleState.SLIDING,
                              VehicleState.AIRBORNE, VehicleState.UNKNOWN):
            return ShiftDecision(reason=f"Shift inhibited in {analysis.state.value}",
                                 blocked_by=analysis.state.value)

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
        if now - self._last_shift_at < c.cooldown_s:
            return ShiftDecision(reason="Shift cooldown", blocked_by="COOLDOWN")

        up_target = frame.gear + 1
        down_target = frame.gear - 1
        up_ratio = profile.gear_ratio_rpm_per_mps.get(up_target) if profile else None
        down_ratio = profile.gear_ratio_rpm_per_mps.get(down_target) if profile else None

        if analysis.state == VehicleState.BRAKING and frame.brake > 0.15:
            if down_ratio and profile and profile.gear_samples.get(down_target, 0) >= 30:
                predicted = self._predict_rpm(frame, down_ratio)
                if predicted and frame.idle_rpm * 1.15 < predicted < frame.engine_max_rpm * 0.88:
                    return self._decision(down_target, "MEASURED BRAKING + SAFE RPM", predicted, 0.75)

        if (analysis.state == VehicleState.ACCELERATING and frame.throttle > 0.72
                and frame.rpm_pct >= c.upshift_rpm_pct):
            if up_ratio and profile and profile.gear_samples.get(up_target, 0) >= 30:
                predicted = self._predict_rpm(frame, up_ratio)
                if predicted and frame.idle_rpm * 1.2 < predicted < frame.engine_max_rpm * 0.94:
                    return self._decision(up_target, "LEARNED UPSHIFT MODEL", predicted, 0.7)
            return ShiftDecision(reason="Upshift threshold reached; waiting for a validated next-gear model",
                                 blocked_by="INSUFFICIENT_CALIBRATION")
        return ShiftDecision(reason="Current gear remains appropriate")

    @staticmethod
    def _predict_rpm(frame: TelemetryFrame, ratio: float | None) -> float | None:
        if ratio is None or not (1.0 < ratio < 2000.0) or frame.speed_mps <= 0:
            return None
        predicted = frame.speed_mps * ratio
        return predicted if predicted > 0 else None

    def _decision(self, target: int, reason: str, predicted: float, confidence: float) -> ShiftDecision:
        return ShiftDecision(requested_gear=target,
                             direction="UP" if reason.startswith(("REDLINE", "LEARNED UPSHIFT")) else "DOWN",
                             reason=reason, predicted_rpm=predicted, confidence=confidence)

    def confirm_sent(self, target_gear: int) -> None:
        self._pending_target = target_gear
        self._pending_since = monotonic()
        self._last_shift_at = self._pending_since

    def cancel_pending(self) -> None:
        self._pending_target = None
        self._pending_since = 0.0
