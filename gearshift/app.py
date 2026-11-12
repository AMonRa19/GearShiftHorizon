from __future__ import annotations

import logging
import os
import socket
import sys
from logging.handlers import RotatingFileHandler
from pathlib import Path
from time import monotonic

from PySide6.QtCore import QObject, QSettings, QThread, QTimer, Signal
from PySide6.QtWidgets import QApplication, QMessageBox

from .analyzer import VehicleStateAnalyzer
from .calibration import CalibrationStore
from .controller import ControllerConfig, ShiftController
from .gui import HudWindow, MainWindow
from .input_win import KeyboardOutput
from .models import ShiftDecision, TelemetryFrame, VehicleAnalysis
from .telemetry import PACKET_SIZE, TelemetryParseError, parse_packet

LOG = logging.getLogger("gearshift")
DEFAULT_UDP_PORT = 5555


class TelemetryWorker(QObject):
    frame_received = Signal(object)
    status_changed = Signal(bool, str)

    def __init__(self, port: int = DEFAULT_UDP_PORT) -> None:
        super().__init__()
        self.port = int(port)
        self._running = True

    def run(self) -> None:
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            sock.bind(("0.0.0.0", self.port))
            sock.settimeout(0.25)
            self.status_changed.emit(False, f"UDP {self.port}: приёмник запущен, ожидание пакетов")
            LOG.info("UDP receiver listening on 0.0.0.0:%s", self.port)
            last_error = None
            last_good_status = 0.0

            while self._running:
                try:
                    packet, sender = sock.recvfrom(4096)
                except socket.timeout:
                    continue

                try:
                    frame = parse_packet(packet)
                except TelemetryParseError as exc:
                    message = (
                        f"UDP-пакет от {sender[0]} получен ({len(packet)} байт), "
                        f"но отклонён: {exc}"
                    )
                    if message != last_error:
                        LOG.warning("%s", message)
                        self.status_changed.emit(False, message)
                        last_error = message
                    continue

                last_error = None
                self.frame_received.emit(frame)
                now = monotonic()
                if now - last_good_status >= 2.0:
                    self.status_changed.emit(
                        True, f"UDP {self.port}: данные FH6 получены от {sender[0]}"
                    )
                    last_good_status = now

        except OSError as exc:
            message = f"Ошибка UDP {self.port}: {exc}"
            self.status_changed.emit(False, message)
            LOG.exception("UDP receiver failed")
        finally:
            sock.close()

    def stop(self) -> None:
        self._running = False


class ApplicationController(QObject):
    def __init__(self, window: MainWindow) -> None:
        super().__init__()
        self.window = window
        self.hud = HudWindow()
        self.preferences = QSettings("AMonRa19", "GearShiftHorizon")
        saved_geometry = self.preferences.value("window/geometry")
        if saved_geometry:
            self.window.restoreGeometry(saved_geometry)
        self.window.hud_scale.setValue(int(self.preferences.value("hud/scale", 100)))
        self.window.hud_opacity.setValue(int(self.preferences.value("hud/opacity", 88)))
        self.hud.set_scale(self.window.hud_scale.value())
        self.hud.set_opacity(self.window.hud_opacity.value())
        self.hud.move(int(self.preferences.value("hud/x", 40)),
                      int(self.preferences.value("hud/y", 40)))
        self.analyzer = VehicleStateAnalyzer()
        self.calibration = CalibrationStore()
        self.shift = ShiftController(ControllerConfig())
        self.keyboard = KeyboardOutput()
        self.last_frame: TelemetryFrame | None = None
        self.last_analysis: VehicleAnalysis | None = None
        self.last_decision: ShiftDecision | None = None
        self.last_profile = None
        self._frames_since_save = 0
        self.automatic = False
        self.worker_status = f"UDP {DEFAULT_UDP_PORT}: ожидание пакетов"
        self.worker_thread = QThread()
        self.worker = TelemetryWorker()
        self.worker.moveToThread(self.worker_thread)
        self.worker_thread.started.connect(self.worker.run)
        self.worker.frame_received.connect(self.on_frame)
        self.worker.status_changed.connect(self.on_status)
        self.worker_thread.start()
        self.window.hud_toggled.connect(self.toggle_hud)
        self.window.automatic_toggled.connect(self.set_automatic)
        self.window.hold_changed.connect(self.set_hold)
        self.window.hud_scale_changed.connect(self.set_hud_scale)
        self.window.hud_opacity_changed.connect(self.set_hud_opacity)
        self.refresh_timer = QTimer(self)
        self.refresh_timer.timeout.connect(self.refresh_ui)
        self.refresh_timer.start(50)

    def on_frame(self, frame: TelemetryFrame) -> None:
        if self.last_frame and frame.timestamp_ms == self.last_frame.timestamp_ms:
            return
        if self.last_frame and frame.timestamp_ms < self.last_frame.timestamp_ms:
            backwards_ms = self.last_frame.timestamp_ms - frame.timestamp_ms
            if backwards_ms < 0x80000000 and backwards_ms <= 5000:
                return
            self.analyzer.reset()
            self.shift.cancel_pending()
        if self.last_frame and (
            self.last_frame.car_ordinal != frame.car_ordinal
            or self.last_frame.race_on != frame.race_on
        ):
            self.analyzer.reset()
            self.shift.cancel_pending()

        analysis = self.analyzer.update(frame)
        profile = (self.calibration.observe(frame, analysis.state.value)
                   if frame.race_on else self.calibration.active())
        decision = self.shift.evaluate(frame, analysis, profile)
        self.last_frame, self.last_analysis, self.last_decision = frame, analysis, decision
        self.last_profile = profile

        if frame.race_on:
            self._frames_since_save += 1
            if self._frames_since_save >= 600:
                self.calibration.save()
                self._frames_since_save = 0

        if self.automatic and decision.requested_gear and decision.direction in ("UP", "DOWN"):
            try:
                shift_count = abs(decision.requested_gear - frame.gear)
                self.keyboard.shift(decision.direction, shift_count)
                self.shift.confirm_sent(decision.requested_gear)
                LOG.info("Shift %s -> gear %s: %s", decision.direction,
                         decision.requested_gear, decision.reason)
            except (OSError, ValueError):
                LOG.exception("Keyboard output failed")
                self.automatic = False
                self.window.auto_check.setChecked(False)
                QMessageBox.warning(
                    self.window, "Keyboard output",
                    "Не удалось отправить команду переключения. "
                    "Проверь назначение клавиш E/Q и активное окно Forza. "
                    "Автоматический режим отключён."
                )

    def on_status(self, connected: bool, message: str) -> None:
        self.worker_status = message
        self.window.connection_label.setText(message)
        if not connected:
            LOG.warning("%s", message)

    def set_automatic(self, enabled: bool) -> None:
        self.automatic = enabled
        LOG.info("Automatic shifting %s", "enabled" if enabled else "disabled")

    def set_hold(self, value: int) -> None:
        self.keyboard.hold_ms = max(10, min(120, value))

    def set_hud_scale(self, value: int) -> None:
        self.hud.set_scale(value)
        self.preferences.setValue("hud/scale", value)

    def set_hud_opacity(self, value: int) -> None:
        self.hud.set_opacity(value)
        self.preferences.setValue("hud/opacity", value)

    def toggle_hud(self, enabled: bool) -> None:
        self.hud.show() if enabled else self.hud.hide()

    def refresh_ui(self) -> None:
        connected = bool(self.last_frame and monotonic() - self.last_frame.received_at < 0.75)
        if not connected and self.automatic:
            self.automatic = False
            self.window.auto_check.setChecked(False)

        frame = self.last_frame if connected else None
        analysis = self.last_analysis if connected else None
        decision = self.last_decision if connected else None
        self.window.update_snapshot(
            frame, analysis, decision, connected,
            self.last_profile if connected else None
        )
        if connected:
            self.window.connection_label.setText(
                f"UDP {self.worker.port}: телеметрия поступает"
            )
        elif self.last_frame:
            self.window.connection_label.setText(
                f"Нет свежих пакетов. Последний статус: {self.worker_status}"
            )
        else:
            self.window.connection_label.setText(self.worker_status)
        self.hud.update_data(frame, analysis, decision)

    def shutdown(self) -> None:
        self.worker.stop()
        self.worker_thread.quit()
        self.worker_thread.wait(1000)
        self.calibration.save()
        self.preferences.setValue("window/geometry", self.window.saveGeometry())
        self.preferences.setValue("hud/x", self.hud.x())
        self.preferences.setValue("hud/y", self.hud.y())
        self.hud.close()


def configure_logging() -> None:
    handlers: list[logging.Handler] = [logging.StreamHandler()]
    try:
        root = Path(os.environ.get("LOCALAPPDATA", str(Path.home()))) / "GearShiftHorizon"
        root.mkdir(parents=True, exist_ok=True)
        handlers.append(RotatingFileHandler(
            root / "gearshift.log", maxBytes=2_000_000, backupCount=3, encoding="utf-8"
        ))
    except OSError:
        pass
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        handlers=handlers,
        force=True,
    )


def main() -> int:
    configure_logging()
    app = QApplication(sys.argv)
    app.setApplicationName("GearShiftHorizon")
    window = MainWindow()
    controller = ApplicationController(window)
    app.aboutToQuit.connect(controller.shutdown)
    window.show()
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
