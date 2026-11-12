from __future__ import annotations

import math
import struct

from .models import TelemetryFrame

PACKET_SIZE = 324


class TelemetryParseError(ValueError):
    pass


def _f32(packet: bytes, offset: int) -> float:
    value = struct.unpack_from("<f", packet, offset)[0]
    if not math.isfinite(value):
        raise TelemetryParseError(f"Non-finite float at byte offset {offset}")
    return value


def _i32(packet: bytes, offset: int) -> int:
    return struct.unpack_from("<i", packet, offset)[0]


def _u32(packet: bytes, offset: int) -> int:
    return struct.unpack_from("<I", packet, offset)[0]


def _floats4(packet: bytes, offset: int) -> tuple[float, float, float, float]:
    values = struct.unpack_from("<4f", packet, offset)
    if not all(math.isfinite(v) for v in values):
        raise TelemetryParseError(f"Non-finite float in four-value field at byte offset {offset}")
    return values


def parse_packet(packet: bytes) -> TelemetryFrame:
    """Parse the official 324-byte FH6 Data Out packet.

    Offsets follow the Forza Horizon 6 Data Out field order. Unknown fields are
    deliberately not assigned invented meanings.
    """
    if len(packet) != PACKET_SIZE:
        raise TelemetryParseError(
            f"Unexpected packet size: {len(packet)} bytes; expected exactly {PACKET_SIZE}"
        )

    f = lambda offset: _f32(packet, offset)
    i = lambda offset: _i32(packet, offset)

    frame = TelemetryFrame(
        race_on=i(0) == 1,
        timestamp_ms=_u32(packet, 4),
        engine_max_rpm=f(8),
        idle_rpm=f(12),
        rpm=f(16),
        accel_xyz=(f(20), f(24), f(28)),
        velocity_xyz=(f(32), f(36), f(40)),
        angular_velocity_xyz=(f(44), f(48), f(52)),
        normalized_suspension_travel=_floats4(packet, 68),
        wheel_slips=_floats4(packet, 84),
        wheel_rotation_speeds=_floats4(packet, 100),
        wheel_on_rumble_strip=tuple(i(o) for o in (116, 120, 124, 128)),
        wheel_in_puddle=tuple(i(o) for o in (132, 136, 140, 144)),
        surface_rumble=_floats4(packet, 148),
        tire_slip_angles=_floats4(packet, 164),
        combined_slips=_floats4(packet, 180),
        suspension_travel_m=_floats4(packet, 196),
        car_ordinal=i(212),
        car_class=i(216),
        pi=i(220),
        drivetrain=i(224),
        cylinders=i(228),
        car_group=_u32(packet, 232),
        smashable_vel_diff=f(236),
        smashable_mass=f(240),
        position_xyz=(f(244), f(248), f(252)),
        speed_mps=f(256),
        power_w=f(260),
        torque_nm=f(264),
        tire_temps=_floats4(packet, 268),
        boost_raw=f(284),
        fuel=f(288),
        distance_traveled=f(292),
        best_lap=f(296),
        last_lap=f(300),
        current_lap=f(304),
        current_race_time=f(308),
        lap_number=struct.unpack_from("<H", packet, 312)[0],
        race_position=packet[314],
        throttle_raw=packet[315],
        brake_raw=packet[316],
        clutch_raw=packet[317],
        handbrake_raw=packet[318],
        gear=packet[319],
        steer=struct.unpack_from("<b", packet, 320)[0],
        normalized_driving_line=struct.unpack_from("<b", packet, 321)[0],
        normalized_ai_brake_difference=struct.unpack_from("<b", packet, 322)[0],
        trailing_byte=packet[323],
    )
    if (frame.engine_max_rpm < 0 or frame.idle_rpm < 0 or frame.rpm < 0
            or frame.speed_mps < -1 or frame.smashable_mass < 0
            or frame.fuel < -0.01 or frame.fuel > 1.01):
        raise TelemetryParseError("Telemetry contains out-of-range core values")
    return frame
