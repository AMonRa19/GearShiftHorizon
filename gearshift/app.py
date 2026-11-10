from __future__ import annotations

import logging
import socket
import sys
from time import monotonic

from PySide6.QtCore import QObject, QThread, QTimer, Signal
from PySide6.QtWidgets import QApplication, QMessageBox

from .analyzer import VehicleStateAnalyzer
from .calibration import CalibrationStore
from .controller import ControllerConfig, ShiftController
from .gui import HudWindow, MainWindow
from .input_win import KeyboardOutput
from .models import ShiftDecision, TelemetryFrame, VehicleAnalysis
from .telemetry import TelemetryParseError, parse_packet

LOG = logging.getLogger("gearshift")


class TelemetryWorker(QObject):
    frame_received = Signal(object)
    status_changed = Signal(bool, str)

    def __init__(self, port: int = 5555) -> None:
        super().__init__()
        self.port = port
        self._running = True

    def run(self) -> None:
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            sock.bind(("0.0.0.0", self.port))
            sock.settimeout(0.25)
            self.status_changed.emit(False, f"Listening on UDP {self.port}")
            while self._running:
                try:
                    packet, _ = sock.recvfrom(2048)
                except socket.timeout:
                    continue
                try:
                    frame = parse_packet(packet)
                except TelemetryParseError as exc:
                    LOG.debug("Discarded telemetry packet: %s", exc)
                    continue
                self.frame_received.emit(frame)
                self.status_changed.emit(True, "Telemetry received")
        except OSError as exc:
            self.status_changed.emit(False, str(exc))
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
        self.analyzer = VehicleStateAnalyzer()
        self.calibration = CalibrationStore()
        self.shift = ShiftController(ControllerConfig())
        self.keyboard = KeyboardOutput()
        self.last_frame: TelemetryFrame | None = None
        self.last_analysis: VehicleAnalysis | None = None
        self.last_decision: ShiftDecision | None = None
        self.automatic = False
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
        self.refresh_timer = QTimer(self)
        self.refresh_timer.timeout.connect(self.refresh_ui)
        self.refresh_timer.start(50)

    def on_frame(self, frame: TelemetryFrame) -> None:
        if self.last_frame and frame.timestamp_ms == self.last_frame.timestamp_ms:
            return
        if self.last_frame and frame.timestamp_ms < self.last_frame.timestamp_ms:
            if self.last_frame.timestamp_ms - frame.timestamp_ms < 0x80000000:
                return
        if self.last_frame and self.last_frame.car_ordinal != frame.car_ordinal:
            self.analyzer.reset()
            self.shift.cancel_pending()
        analysis = self.analyzer.update(frame)
        profile = self.calibration.observe(frame, analysis.state.value)
        decision = self.shift.evaluate(frame, analysis, profile)
        self.last_frame, self.last_analysis, self.last_decision = frame, analysis, decision

        if self.automatic and decision.requested_gear and decision.direction in ("UP", "DOWN"):
            try:
                self.keyboard.shift(decision.direction)
                self.shift.confirm_sent(decision.requested_gear)
                LOG.info("Shift %s -> gear %s: %s", decision.direction,
                         decision.requested_gear, decision.reason)
            except OSError:
                LOG.exception("Keyboard output failed")
                self.automatic = False
                self.window.auto_check.setChecked(False)
                QMessageBox.warning(self.window, "Keyboard output",
                                    "Не удалось отправить клавишу. Автоматический режим отключён.")

    def on_status(self, connected: bool, message: str) -> None:
        del connected
        self.window.connection_label.setText(message)

    def set_automatic(self, enabled: bool) -> None:
        self.automatic = enabled

    def set_hold(self, value: int) -> None:
        self.keyboard.hold_ms = max(10, min(120, value))

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
        self.window.update_snapshot(frame, analysis, decision, connected)
        self.hud.update_data(frame, analysis, decision)

    def shutdown(self) -> None:
        self.worker.stop()
        self.worker_thread.quit()
        self.worker_thread.wait(1000)
        self.calibration.save()
        self.hud.close()


def main() -> int:
    logging.basicConfig(level=logging.INFO,
                        format="%(asctime)s %(levelname)s %(name)s: %(message)s")
    app = QApplication(sys.argv)
    app.setApplicationName("GearShiftHorizon")
    window = MainWindow()
    controller = ApplicationController(window)
    app.aboutToQuit.connect(controller.shutdown)
    window.show()
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
