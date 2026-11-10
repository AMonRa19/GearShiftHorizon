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
    yaw_pitch_roll: tuple[float, float, float] = (0.0, 0.0, 0.0)
    normalized_suspension_travel: tuple[float, float, float, float] = (0.0, 0.0, 0.0, 0.0)
    wheel_rotation_speeds: tuple[float, float, float, float] = (0.0, 0.0, 0.0, 0.0)
    wheel_on_rumble_strip: tuple[int, int, int, int] = (0, 0, 0, 0)
    wheel_in_puddle: tuple[int, int, int, int] = (0, 0, 0, 0)
    surface_rumble: tuple[float, float, float, float] = (0.0, 0.0, 0.0, 0.0)
    tire_slip_angles: tuple[float, float, float, float] = (0.0, 0.0, 0.0, 0.0)
    combined_slips: tuple[float, float, float, float] = (0.0, 0.0, 0.0, 0.0)
    suspension_travel_m: tuple[float, float, float, float] = (0.0, 0.0, 0.0, 0.0)
    car_group: int = 0
    smashable_vel_diff: float = 0.0
    smashable_mass: float = 0.0
    position_xyz: tuple[float, float, float] = (0.0, 0.0, 0.0)
    tire_temps: tuple[float, float, float, float] = (0.0, 0.0, 0.0, 0.0)
    fuel: float = 0.0
    distance_traveled: float = 0.0
    best_lap: float = 0.0
    last_lap: float = 0.0
    current_lap: float = 0.0
    current_race_time: float = 0.0
    lap_number: int = 0
    race_position: int = 0
    handbrake_raw: int = 0
    steer: int = 0
    normalized_driving_line: int = 0
    normalized_ai_brake_difference: int = 0
    trailing_byte: int = 0
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

    @property
    def max_abs_combined_slip(self) -> float:
        return max(abs(v) for v in self.combined_slips)

    @property
    def lateral_g_estimate(self) -> float:
        # Official FH6 Data Out axis convention: X is right, Y up, Z forward.
        return self.accel_xyz[0] / 9.80665


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
