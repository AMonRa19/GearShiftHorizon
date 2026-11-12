from time import monotonic

from gearshift.calibration import CarProfile
from gearshift.controller import ShiftController
from gearshift.models import TelemetryFrame, VehicleAnalysis, VehicleState


def frame(*, rpm=6000, gear=4, speed=30, brake=0.0, throttle=0.8, handbrake=0.0):
    return TelemetryFrame(
        race_on=True, timestamp_ms=100, engine_max_rpm=9000, idle_rpm=900, rpm=rpm,
        accel_xyz=(0, 0, 0), velocity_xyz=(0, 0, 0), angular_velocity_xyz=(0, 0, 0),
        wheel_slips=(0.01, 0.01, 0.01, 0.01), car_ordinal=7, car_class=6, pi=900,
        drivetrain=2, cylinders=8, speed_mps=speed, power_w=200000, torque_nm=500,
        boost_raw=0, throttle_raw=int(throttle * 255), brake_raw=int(brake * 255),
        clutch_raw=0, gear=gear, handbrake_raw=int(handbrake * 255),
    )


def test_brake_pedal_alone_does_not_request_downshift_without_deceleration_state():
    result = ShiftController().evaluate(
        frame(brake=1.0),
        VehicleAnalysis(state=VehicleState.CRUISING, speed_kmh=108, brake=1.0),
        None,
    )
    assert result.requested_gear is None


def test_handbrake_blocks_all_shifts_even_at_redline():
    result = ShiftController().evaluate(
        frame(rpm=8900, gear=4, throttle=1.0, handbrake=1.0),
        VehicleAnalysis(state=VehicleState.ACCELERATING, speed_kmh=108,
                        throttle=1.0, rpm_pct=0.99),
        None,
    )
    assert result.requested_gear is None
    assert result.blocked_by == "HANDBRAKE"


def test_sliding_state_blocks_emergency_upshift():
    result = ShiftController().evaluate(
        frame(rpm=8900, gear=4),
        VehicleAnalysis(state=VehicleState.SLIDING, speed_kmh=108, max_wheel_slip=0.5),
        None,
    )
    assert result.requested_gear is None
    assert result.blocked_by == "SLIDING"


def test_braking_downshift_uses_safe_learned_ratio():
    profile = CarProfile(
        car_ordinal=7,
        gear_ratio_rpm_per_mps={3: 100.0},
        gear_samples={3: 30},
    )
    result = ShiftController().evaluate(
        frame(rpm=3000, gear=4, speed=30, brake=1.0, throttle=0.0),
        VehicleAnalysis(state=VehicleState.BRAKING, speed_kmh=108, brake=1.0),
        profile,
    )
    assert result.direction == "DOWN"
    assert result.requested_gear == 3
    assert result.predicted_rpm == 3000


def test_kickdown_on_high_throttle_and_low_rpm():
    profile = CarProfile(
        car_ordinal=7,
        gear_ratio_rpm_per_mps={3: 150.0},
        gear_samples={3: 30},
    )
    result = ShiftController().evaluate(
        frame(rpm=4000, gear=4, speed=30, throttle=0.95),
        VehicleAnalysis(state=VehicleState.ACCELERATING, speed_kmh=108, throttle=0.95),
        profile,
    )
    assert result.direction == "DOWN"
    assert result.requested_gear == 3


def test_upshift_requires_acceleration_and_throttle():
    result = ShiftController().evaluate(
        frame(rpm=8300, gear=4, throttle=0.95),
        VehicleAnalysis(state=VehicleState.ACCELERATING, speed_kmh=108,
                        throttle=0.95, longitudinal_accel_mps2=1.0),
        None,
    )
    assert result.direction == "UP"
    assert result.requested_gear == 5
    assert "FALLBACK" in result.reason


def test_high_rpm_while_braking_does_not_upshift():
    result = ShiftController().evaluate(
        frame(rpm=8900, gear=4, brake=1.0, throttle=0.0),
        VehicleAnalysis(state=VehicleState.BRAKING, speed_kmh=108, brake=1.0),
        None,
    )
    assert result.requested_gear is None


def test_shift_confirmation_timeout_enters_retry_guard():
    controller = ShiftController()
    controller.confirm_sent(5)
    controller._pending_since = monotonic() - 2.0
    result = controller.evaluate(
        frame(), VehicleAnalysis(state=VehicleState.CRUISING, speed_kmh=108), None
    )
    assert result.blocked_by == "SHIFT_NOT_CONFIRMED"


def test_routine_shifts_are_blocked_during_impact_recovery():
    result = ShiftController().evaluate(
        frame(),
        VehicleAnalysis(state=VehicleState.IMPACT, speed_kmh=108, smashable_vel_diff=4.0),
        None,
    )
    assert result.requested_gear is None
    assert result.blocked_by == "IMPACT"
