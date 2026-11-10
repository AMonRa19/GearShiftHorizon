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


def parse_packet(packet: bytes) -> TelemetryFrame:
    """Parse the 324-byte FH telemetry layout used by the previous project parser.

    Only offsets already used by the Rust 0.19 parser are interpreted here. Unknown
    bytes are intentionally not guessed or exposed as meaningful vehicle metrics.
    """
    if len(packet) < PACKET_SIZE:
        raise TelemetryParseError(f"Packet too short: {len(packet)} bytes; expected {PACKET_SIZE}")

    f = lambda o: _f32(packet, o)
    i = lambda o: _i32(packet, o)
    frame = TelemetryFrame(
        race_on=i(0) != 0,
        timestamp_ms=struct.unpack_from("<I", packet, 4)[0],
        engine_max_rpm=f(8),
        idle_rpm=f(12),
        rpm=f(16),
        accel_xyz=(f(20), f(24), f(28)),
        velocity_xyz=(f(32), f(36), f(40)),
        angular_velocity_xyz=(f(44), f(48), f(52)),
        wheel_slips=(f(84), f(88), f(92), f(96)),
        car_ordinal=i(212),
        car_class=i(216),
        pi=i(220),
        drivetrain=i(224),
        cylinders=i(228),
        speed_mps=f(256),
        power_w=f(260),
        torque_nm=f(264),
        boost_raw=f(284),
        throttle_raw=packet[315],
        brake_raw=packet[316],
        clutch_raw=packet[317],
        gear=packet[319],
    )
    if frame.engine_max_rpm < 0 or frame.rpm < 0 or frame.speed_mps < -1:
        raise TelemetryParseError("Telemetry contains out-of-range core values")
    return frame
