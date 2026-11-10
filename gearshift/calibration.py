from __future__ import annotations

import json
import logging
from dataclasses import asdict, dataclass, field
from pathlib import Path

from .models import TelemetryFrame


@dataclass
class CarProfile:
    car_ordinal: int
    car_class: int = 0
    pi: int = 0
    drivetrain: int = 0
    samples: int = 0
    gear_ratio_rpm_per_mps: dict[int, float] = field(default_factory=dict)
    gear_samples: dict[int, int] = field(default_factory=dict)
    rpm_max_ema: float = 0.0
    power_curve: dict[int, list[float]] = field(default_factory=dict)

    @property
    def ratio_confidence(self) -> float:
        learned = sum(1 for n in self.gear_samples.values() if n >= 30)
        return min(1.0, learned / 6.0)

    def estimated_power_kw(self, rpm_pct: float) -> float | None:
        """Interpolate only across buckets with enough real telemetry samples."""
        points = []
        for bucket, samples in self.power_curve.items():
            if len(samples) >= 12:
                values = [v for v in samples if v > 0]
                if values:
                    points.append((bucket / 20.0, sum(values) / len(values)))
        points.sort()
        if len(points) < 3:
            return None
        x = max(0.0, min(1.05, rpm_pct))
        if x <= points[0][0]:
            return points[0][1]
        if x >= points[-1][0]:
            return points[-1][1]
        for (x0, y0), (x1, y1) in zip(points, points[1:]):
            if x0 <= x <= x1:
                fraction = (x - x0) / max(0.001, x1 - x0)
                return y0 + (y1 - y0) * fraction
        return None


class CalibrationStore:
    def __init__(self, path: str = "tcu_calibration.json") -> None:
        self.path = Path(path)
        self.profiles: dict[int, CarProfile] = {}
        self.active_car: int | None = None
        self._previous: TelemetryFrame | None = None
        self.load()

    def observe(self, frame: TelemetryFrame, state_name: str) -> CarProfile:
        profile = self.profiles.setdefault(
            frame.car_ordinal,
            CarProfile(frame.car_ordinal, frame.car_class, frame.pi, frame.drivetrain),
        )
        profile.car_class, profile.pi, profile.drivetrain = frame.car_class, frame.pi, frame.drivetrain
        profile.samples += 1
        profile.rpm_max_ema = frame.engine_max_rpm if profile.rpm_max_ema <= 0 else (
            profile.rpm_max_ema * 0.97 + frame.engine_max_rpm * 0.03
        )
        previous = self._previous
        if (previous and previous.car_ordinal == frame.car_ordinal
                and previous.gear == frame.gear and frame.gear > 0
                and state_name == "ACCELERATING" and frame.throttle > 0.65
                and frame.max_abs_slip < 0.12 and frame.speed_mps > 4.0):
            ratio = frame.rpm / frame.speed_mps if frame.speed_mps > 0 else 0.0
            if 1.0 < ratio < 2000.0:
                n = profile.gear_samples.get(frame.gear, 0)
                old = profile.gear_ratio_rpm_per_mps.get(frame.gear, ratio)
                alpha = 1.0 / min(n + 1, 30)
                profile.gear_ratio_rpm_per_mps[frame.gear] = old * (1 - alpha) + ratio * alpha
                profile.gear_samples[frame.gear] = n + 1
        if (frame.rpm > 500 and frame.power_w > 0 and frame.engine_max_rpm > 0
                and frame.throttle > 0.55 and frame.max_abs_slip < 0.12):
            bucket = int((frame.rpm / frame.engine_max_rpm) * 20)
            bucket = max(0, min(20, bucket))
            samples = profile.power_curve.setdefault(bucket, [])
            samples.append(frame.power_w / 1000.0)
            if len(samples) > 120:
                del samples[:-120]
        self.active_car = frame.car_ordinal
        self._previous = frame
        return profile

    def active(self) -> CarProfile | None:
        return self.profiles.get(self.active_car) if self.active_car is not None else None

    def save(self) -> None:
        payload = {
            "active_car": self.active_car,
            "profiles": {str(k): asdict(v) for k, v in self.profiles.items()},
        }
        try:
            self.path.write_text(json.dumps(payload, indent=2), encoding="utf-8")
        except OSError:
            logging.getLogger("gearshift").exception("Could not save calibration file %s", self.path)

    def load(self) -> None:
        if not self.path.exists():
            return
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
            self.active_car = data.get("active_car")
            for key, raw in data.get("profiles", {}).items():
                raw["gear_ratio_rpm_per_mps"] = {int(k): v for k, v in raw.get("gear_ratio_rpm_per_mps", {}).items()}
                raw["gear_samples"] = {int(k): v for k, v in raw.get("gear_samples", {}).items()}
                raw["power_curve"] = {int(k): v for k, v in raw.get("power_curve", {}).items()}
                self.profiles[int(key)] = CarProfile(**raw)
        except (OSError, ValueError, TypeError, KeyError):
            self.profiles.clear()
            self.active_car = None
