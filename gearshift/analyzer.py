from __future__ import annotations

from collections import deque
from time import monotonic

from .models import TelemetryFrame, VehicleAnalysis, VehicleState


class VehicleStateAnalyzer:
    """Classifies motion from a time series, not from pedal input alone."""

    def __init__(self) -> None:
        self._samples: deque[tuple[float, float]] = deque(maxlen=12)
        self._state = VehicleState.UNKNOWN
        self._state_since = monotonic()
        self._sliding_since: float | None = None
        self._last_race_on = False
        self._analysis = VehicleAnalysis()

    def reset(self) -> None:
        self._samples.clear()
        self._state = VehicleState.UNKNOWN
        self._state_since = monotonic()
        self._sliding_since = None
        self._last_race_on = False
        self._analysis = VehicleAnalysis()

    def update(self, frame: TelemetryFrame) -> VehicleAnalysis:
        now = frame.received_at
        speed = max(0.0, frame.speed_kmh)
        self._samples.append((now, speed))
        previous = self._samples[-2] if len(self._samples) >= 2 else None
        dt = max(0.001, now - previous[0]) if previous else None
        delta = speed - previous[1] if previous else 0.0
        accel = (delta / 3.6) / dt if dt else None

        slip = frame.max_abs_slip
        stopped = speed < 2.0
        sliding = slip >= 0.28
        accelerating = (accel is not None and accel > 0.65 and delta > 0.08
                        and frame.throttle > 0.12)
        genuinely_braking = (accel is not None and accel < -0.75 and delta < -0.08)
        coasting = (accel is not None and abs(accel) < 0.45)

        if not frame.race_on:
            target, confidence, reason = VehicleState.UNKNOWN, 0.0, "Race telemetry inactive"
        elif stopped:
            target, confidence, reason = VehicleState.STOPPED, 0.98, "Vehicle speed is near zero"
        elif sliding:
            target, confidence, reason = VehicleState.SLIDING, min(0.95, 0.55 + slip), "Wheel-slip telemetry exceeds threshold"
        elif genuinely_braking:
            target, confidence = VehicleState.BRAKING, min(0.95, 0.55 + abs(accel or 0) / 8)
            reason = "Measured speed is decreasing"
        elif accelerating:
            target, confidence = VehicleState.ACCELERATING, min(0.95, 0.55 + (accel or 0) / 8)
            reason = "Measured speed is increasing under throttle"
        elif self._state in (VehicleState.SLIDING, VehicleState.BRAKING) and coasting:
            target, confidence, reason = VehicleState.RECOVERING, 0.68, "Speed and slip are stabilizing"
        elif coasting:
            target, confidence, reason = VehicleState.CRUISING, 0.72, "Speed is relatively stable"
        else:
            target, confidence, reason = VehicleState.CRUISING, 0.48, "Insufficient motion evidence"

        # Hysteresis: require repeated evidence before leaving a stable state.
        if target != self._state:
            if target in (VehicleState.STOPPED, VehicleState.SLIDING, VehicleState.BRAKING):
                self._set_state(target, now)
            elif confidence >= 0.60:
                self._set_state(target, now)

        state = self._state
        analysis = VehicleAnalysis(
            state=state,
            confidence=confidence if state == target else 0.35,
            speed_kmh=speed,
            speed_delta_kmh=delta,
            longitudinal_accel_mps2=accel,
            throttle=frame.throttle,
            brake=frame.brake,
            max_wheel_slip=slip,
            rpm_pct=frame.rpm_pct,
            state_duration_s=max(0.0, now - self._state_since),
            reason=reason if state == target else f"Transition filter: {state.value}",
            updated_at=now,
        )
        self._analysis = analysis
        self._last_race_on = frame.race_on
        return analysis

    def _set_state(self, state: VehicleState, now: float) -> None:
        self._state = state
        self._state_since = now
