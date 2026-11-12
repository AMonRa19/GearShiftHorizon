from time import monotonic

from gearshift.calibration import CarProfile
from gearshift.controller import ShiftController
from gearshift.models import TelemetryFrame, VehicleAnalysis, VehicleState


def frame(*, rpm=6000, gear=4, speed=30, brake=0.0, throttle=0.8):
    return TelemetryFrame(
        race_on=True, timestamp_ms=100, engine_max_rpm=9000, idle_rpm=900, rpm=rpm,
        accel_xyz=(0, 0, 0), velocity_xyz=(0, 0, 0), angular_velocity_xyz=(0, 0, 0),
        wheel_slips=(0.01, 0.01, 0.01, 0.01), car_ordinal=7, car_class=6, pi=900,
        drivetrain=2, cylinders=8, speed_mps=speed, power_w=200000, torque_nm=500,
        boost_raw=0, throttle_raw=int(throttle * 255), brake_raw=int(brake * 255),
        clutch_raw=0, gear=gear,
    )


def test_brake_pedal_alone_does_not_request_downshift():
    controller = ShiftController()
    result = controller.evaluate(
        frame(brake=1.0),
        VehicleAnalysis(state=VehicleState.CRUISING, speed_kmh=108, brake=1.0),
        None,
    )
    assert result.requested_gear is None


def test_redline_guard_preempts_sliding_lock_without_learned_ratio():
    controller = ShiftController()
    result = controller.evaluate(
        frame(rpm=8900, gear=4),
        VehicleAnalysis(state=VehicleState.SLIDING, speed_kmh=108, max_wheel_slip=0.5),
        None,
    )
    assert result.direction == "UP"
    assert result.requested_gear == 5
    assert "REDLINE SAFETY" in result.reason


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


def test_upshift_uses_conservative_one_step_fallback_before_calibration():
    result = ShiftController().evaluate(
        frame(rpm=8300, gear=4),
        VehicleAnalysis(state=VehicleState.ACCELERATING, speed_kmh=108,
                        throttle=0.95, longitudinal_accel_mps2=1.0),
        None,
    )
    assert result.direction == "UP"
    assert result.requested_gear == 5
    assert "FALLBACK" in result.reason


def test_vehicle_analysis_exposes_lateral_g_for_live_dashboard():
    analysis = VehicleAnalysis(lateral_accel_raw=9.80665)
    assert analysis.lateral_g == 1.0
