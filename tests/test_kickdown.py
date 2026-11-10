from gearshift.calibration import CarProfile
from gearshift.controller import ShiftController
from gearshift.models import TelemetryFrame, VehicleAnalysis, VehicleState


def frame(*, rpm=2500, gear=5, speed=30, brake=0.0, throttle=0.95):
    return TelemetryFrame(
        race_on=True, timestamp_ms=100, engine_max_rpm=9000, idle_rpm=900, rpm=rpm,
        accel_xyz=(0, 0, 0), velocity_xyz=(0, 0, 0), angular_velocity_xyz=(0, 0, 0),
        wheel_slips=(0.01, 0.01, 0.01, 0.01), car_ordinal=7, car_class=6, pi=900,
        drivetrain=2, cylinders=8, speed_mps=speed, power_w=200000, torque_nm=500,
        boost_raw=0, throttle_raw=int(throttle * 255), brake_raw=int(brake * 255),
        clutch_raw=0, gear=gear,
    )


def test_kickdown_requires_learned_safe_target_gear():
    profile = CarProfile(car_ordinal=7)
    profile.gear_ratio_rpm_per_mps[4] = 70.0
    profile.gear_samples[4] = 40
    result = ShiftController().evaluate(
        frame(), VehicleAnalysis(state=VehicleState.ACCELERATING, speed_kmh=108,
                                 throttle=0.95, longitudinal_accel_mps2=1.0), profile
    )
    assert result.direction == "DOWN"
    assert result.requested_gear == 4
    assert result.predicted_rpm == 2100
