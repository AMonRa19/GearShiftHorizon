from __future__ import annotations

from collections import deque
from time import monotonic

from .models import TelemetryFrame, VehicleAnalysis, VehicleState


class VehicleStateAnalyzer:
    """Classifies vehicle motion from filtered time-series evidence."""

    def __init__(self) -> None:
        self._samples: deque[tuple[float, float]] = deque(maxlen=12)
        self._state = VehicleState.UNKNOWN
        self._state_since = monotonic()
        self._candidate: VehicleState | None = None
        self._candidate_count = 0
        self._filtered_accel: float | None = None

    def reset(self) -> None:
        self._samples.clear()
        self._state = VehicleState.UNKNOWN
        self._state_since = monotonic()
        self._candidate = None
        self._candidate_count = 0
        self._filtered_accel = None

    def update(self, frame: TelemetryFrame) -> VehicleAnalysis:
        now = frame.received_at
        speed = max(0.0, frame.speed_kmh)
        self._samples.append((now, speed))

        # Compare against a short multi-frame baseline to reduce one-packet noise.
        previous = self._samples[-4] if len(self._samples) >= 4 else (
            self._samples[-2] if len(self._samples) >= 2 else None
        )
        dt = max(0.01, now - previous[0]) if previous else None
        delta = speed - previous[1] if previous else 0.0
        raw_accel = (delta / 3.6) / dt if dt else None
        if raw_accel is not None:
            self._filtered_accel = (raw_accel if self._filtered_accel is None
                                    else self._filtered_accel * 0.65 + raw_accel * 0.35)
        accel = self._filtered_accel

        slip = max(frame.max_abs_slip, frame.max_abs_combined_slip)
        stopped = speed < 2.0
        sliding = slip >= 0.28
        impact = (frame.smashable_vel_diff >= 1.5 and frame.smashable_mass > 0.0
                  and delta <= -0.8)
        airborne_candidate = (
            speed > 20.0 and frame.normalized_suspension_travel is not None
            and all(value <= 0.035 for value in frame.normalized_suspension_travel)
        )
        accelerating = (accel is not None and accel > 0.65 and delta > 0.08
                        and frame.throttle > 0.12)
        genuinely_braking = (accel is not None and accel < -0.75 and delta < -0.08)
        coasting = accel is not None and abs(accel) < 0.45

        if not frame.race_on:
            target, confidence, reason = VehicleState.UNKNOWN, 0.0, "Race telemetry inactive"
        elif impact:
            target, confidence = VehicleState.IMPACT, min(0.99, 0.65 + frame.smashable_vel_diff / 10)
            reason = f"Collision telemetry: velocity loss {frame.smashable_vel_diff:.1f} m/s"
        elif stopped:
            target, confidence, reason = VehicleState.STOPPED, 0.98, "Vehicle speed is near zero"
        elif airborne_candidate:
            target, confidence, reason = VehicleState.AIRBORNE, 0.72, "All four suspension sensors indicate near-full extension"
        elif sliding:
            target, confidence = VehicleState.SLIDING, min(0.95, 0.55 + slip)
            reason = "Wheel-slip telemetry exceeds threshold"
        elif genuinely_braking:
            target, confidence = VehicleState.BRAKING, min(0.95, 0.55 + abs(accel or 0) / 8)
            reason = "Measured speed is decreasing"
        elif accelerating:
            target, confidence = VehicleState.ACCELERATING, min(0.95, 0.55 + (accel or 0) / 8)
            reason = "Measured speed is increasing under throttle"
        elif self._state in (VehicleState.SLIDING, VehicleState.BRAKING, VehicleState.IMPACT,
                             VehicleState.AIRBORNE) and coasting and slip < 0.18:
            target, confidence, reason = VehicleState.RECOVERING, 0.68, "Speed, slip and suspension are stabilizing"
        elif coasting:
            target, confidence, reason = VehicleState.CRUISING, 0.72, "Speed is relatively stable"
        else:
            target, confidence, reason = VehicleState.CRUISING, 0.48, "Insufficient motion evidence"

        # Severe events react immediately; routine transitions need two consecutive
        # frames of evidence to prevent state flicker from packet noise.
        immediate = (target in (VehicleState.UNKNOWN, VehicleState.STOPPED, VehicleState.IMPACT)
                     or (target == VehicleState.SLIDING and slip >= 0.45)
                     or (target == VehicleState.BRAKING and abs(accel or 0) >= 3.0))
        if target == self._state:
            self._candidate = None
            self._candidate_count = 0
        elif immediate:
            self._set_state(target, now)
        elif confidence >= 0.60:
            if target == self._candidate:
                self._candidate_count += 1
            else:
                self._candidate = target
                self._candidate_count = 1
            if self._candidate_count >= 2:
                self._set_state(target, now)
                self._candidate = None
                self._candidate_count = 0

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
            lateral_g=frame.lateral_g_estimate,
            smashable_vel_diff=frame.smashable_vel_diff,
            steering_input=frame.steer / 127.0,
            state_duration_s=max(0.0, now - self._state_since),
            reason=reason if state == target else f"Transition filter: {state.value}",
            updated_at=now,
        )
        return analysis

    def _set_state(self, state: VehicleState, now: float) -> None:
        self._state = state
        self._state_since = now
