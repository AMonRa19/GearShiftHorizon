from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum
from time import monotonic


class VehicleState(StrEnum):
    UNKNOWN = "UNKNOWN"
    STOPPED = "STOPPED"
    ACCELERATING = "ACCELERATING"
    CRUISING = "CRUISING"
    BRAKING = "BRAKING"
    SLIDING = "SLIDING"
    RECOVERING = "RECOVERING"
    AIRBORNE = "AIRBORNE"


@dataclass(slots=True)
class TelemetryFrame:
    race_on: bool
    timestamp_ms: int
    engine_max_rpm: float
    idle_rpm: float
    rpm: float
    accel_xyz: tuple[float, float, float]
    velocity_xyz: tuple[float, float, float]
    angular_velocity_xyz: tuple[float, float, float]
    wheel_slips: tuple[float, float, float, float]
    car_ordinal: int
    car_class: int
    pi: int
    drivetrain: int
    cylinders: int
    speed_mps: float
    power_w: float
    torque_nm: float
    boost_raw: float
    throttle_raw: int
    brake_raw: int
    clutch_raw: int
    gear: int
    received_at: float = field(default_factory=monotonic)

    @property
    def speed_kmh(self) -> float:
        if 0.0 <= self.speed_mps < 200.0:
            return self.speed_mps * 3.6
        return sum(v * v for v in self.velocity_xyz) ** 0.5 * 3.6

    @property
    def throttle(self) -> float:
        return self.throttle_raw / 255.0

    @property
    def brake(self) -> float:
        return self.brake_raw / 255.0

    @property
    def rpm_pct(self) -> float:
        return self.rpm / self.engine_max_rpm if self.engine_max_rpm > 0 else 0.0

    @property
    def max_abs_slip(self) -> float:
        return max(abs(v) for v in self.wheel_slips)


@dataclass(slots=True)
class VehicleAnalysis:
    state: VehicleState = VehicleState.UNKNOWN
    confidence: float = 0.0
    speed_kmh: float = 0.0
    speed_delta_kmh: float = 0.0
    longitudinal_accel_mps2: float | None = None
    throttle: float = 0.0
    brake: float = 0.0
    max_wheel_slip: float = 0.0
    rpm_pct: float = 0.0
    state_duration_s: float = 0.0
    reason: str = "Waiting for telemetry"
    updated_at: float = field(default_factory=monotonic)


@dataclass(slots=True)
class ShiftDecision:
    requested_gear: int | None = None
    direction: str = "HOLD"
    reason: str = "No shift required"
    predicted_rpm: float | None = None
    confidence: float = 0.0
    blocked_by: str | None = None
    created_at: float = field(default_factory=monotonic)
