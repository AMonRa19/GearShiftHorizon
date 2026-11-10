import struct
from dataclasses import replace

import pytest

from gearshift.analyzer import VehicleStateAnalyzer
from gearshift.models import TelemetryFrame, VehicleState
from gearshift.telemetry import PACKET_SIZE, TelemetryParseError, parse_packet


def make_frame(*, speed=20.0, throttle=0.8, brake=0.0, slip=0.02, race=True, timestamp=1):
    return TelemetryFrame(
        race_on=race, timestamp_ms=timestamp, engine_max_rpm=9000, idle_rpm=900,
        rpm=6000, accel_xyz=(0.0, 0.0, 0.0), velocity_xyz=(0.0, 0.0, 0.0),
        angular_velocity_xyz=(0.0, 0.0, 0.0), wheel_slips=(slip, slip, slip, slip),
        car_ordinal=123, car_class=6, pi=900, drivetrain=2, cylinders=8,
        speed_mps=speed, power_w=200000, torque_nm=500, boost_raw=0,
        throttle_raw=int(throttle * 255), brake_raw=int(brake * 255),
        clutch_raw=0, gear=4,
    )


def test_parser_reads_known_fields():
    packet = bytearray(PACKET_SIZE)
    struct.pack_into("<iIfff", packet, 0, 1, 1234, 9000.0, 900.0, 6000.0)
    struct.pack_into("<f", packet, 256, 50.0)
    struct.pack_into("<f", packet, 236, 2.5)
    struct.pack_into("<f", packet, 268, 92.0)
    struct.pack_into("<H", packet, 312, 3)
    packet[314], packet[318] = 2, 255
    struct.pack_into("<b", packet, 320, -127)
    packet[315], packet[316], packet[317], packet[319] = 255, 0, 0, 5
    frame = parse_packet(bytes(packet))
    assert frame.race_on
    assert frame.timestamp_ms == 1234
    assert frame.speed_kmh == pytest.approx(180.0)
    assert frame.gear == 5
    assert frame.throttle == pytest.approx(1.0)
    assert frame.smashable_vel_diff == pytest.approx(2.5)
    assert frame.tire_temps[0] == pytest.approx(92.0)
    assert frame.lap_number == 3
    assert frame.race_position == 2
    assert frame.handbrake_raw == 255
    assert frame.steer == -127


def test_parser_rejects_short_packet():
    with pytest.raises(TelemetryParseError):
        parse_packet(b"short")


def test_state_uses_measured_speed_change_not_brake_pedal_alone():
    analyzer = VehicleStateAnalyzer()
    analyzer.update(make_frame(speed=20, brake=1.0, timestamp=100))
    result = analyzer.update(make_frame(speed=20, brake=1.0, timestamp=116))
    assert result.state != VehicleState.BRAKING


def test_state_detects_measured_deceleration():
    analyzer = VehicleStateAnalyzer()
    analyzer.update(make_frame(speed=30, timestamp=100))
    result = analyzer.update(make_frame(speed=20, brake=0.8, timestamp=116))
    assert result.state == VehicleState.BRAKING


def test_state_detects_sliding():
    analyzer = VehicleStateAnalyzer()
    analyzer.update(make_frame(speed=30, timestamp=100))
    result = analyzer.update(make_frame(speed=29, slip=0.5, timestamp=116))
    assert result.state == VehicleState.SLIDING



def test_state_detects_collision_from_documented_impact_fields():
    analyzer = VehicleStateAnalyzer()
    analyzer.update(make_frame(speed=30, timestamp=100))
    hit = replace(make_frame(speed=20, timestamp=116),
                  smashable_vel_diff=4.0, smashable_mass=80.0)
    result = analyzer.update(hit)
    assert result.state == VehicleState.IMPACT


def test_airborne_is_inferred_only_when_suspension_data_is_present():
    analyzer = VehicleStateAnalyzer()
    airborne = replace(make_frame(speed=30, timestamp=100),
                       normalized_suspension_travel=(0.01, 0.01, 0.01, 0.01))
    analyzer.update(airborne)
    result = analyzer.update(replace(airborne, timestamp_ms=116))
    assert result.state == VehicleState.AIRBORNE
