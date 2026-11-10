from __future__ import annotations

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QColor, QFont, QPainter, QPen
from PySide6.QtWidgets import (
    QCheckBox, QFrame, QGridLayout, QHBoxLayout, QLabel, QMainWindow,
    QPushButton, QSlider, QVBoxLayout, QWidget,
)

DARK = "#101722"
PANEL = "#182332"
TEXT = "#eaf1fa"
MUTED = "#91a2b8"
ACCENT = "#36b7f4"
GREEN = "#35d49a"
AMBER = "#ffbf5b"
RED = "#ff5e67"


class Tachometer(QWidget):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.rpm_pct = 0.0
        self.rpm = 0
        self.gear = 0
        self.speed = 0.0
        self.setMinimumSize(120, 80)

    def set_values(self, frame) -> None:
        if frame:
            self.rpm_pct = max(0.0, min(1.15, frame.rpm_pct))
            self.rpm = int(frame.rpm)
            self.gear = frame.gear
            self.speed = frame.speed_kmh
        else:
            self.rpm_pct = self.rpm = 0
        self.update()

    def paintEvent(self, event) -> None:
        del event
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        rect = self.rect().adjusted(18, 16, -18, -12)
        center = rect.center()
        radius = min(rect.width() * 0.43, rect.height() * 0.82)
        arc = rect.adjusted(rect.width()/2-radius, rect.height()/2-radius,
                            -(rect.width()/2-radius), -(rect.height()/2-radius))
        arc.setHeight(radius * 2)
        arc.setWidth(radius * 2)
        arc.moveCenter(center)
        p.setPen(QPen(QColor("#34465d"), 13, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap))
        p.drawArc(arc, 225 * 16, -270 * 16)
        color = QColor(GREEN if self.rpm_pct < .82 else AMBER if self.rpm_pct < .94 else RED)
        p.setPen(QPen(color, 13, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap))
        p.drawArc(arc, 225 * 16, int(-270 * 16 * min(1.0, self.rpm_pct)))
        p.setPen(QColor(TEXT))
        p.setFont(QFont("Segoe UI", 22, QFont.Weight.Bold))
        p.drawText(rect.adjusted(0, 28, 0, -18), Qt.AlignmentFlag.AlignCenter, f"{self.rpm:,}")
        p.setFont(QFont("Segoe UI", 9))
        p.setPen(QColor(MUTED))
        p.drawText(rect.adjusted(0, 76, 0, -4), Qt.AlignmentFlag.AlignCenter, "RPM")
        p.setFont(QFont("Segoe UI", 19, QFont.Weight.Bold))
        p.setPen(QColor(ACCENT))
        p.drawText(rect.adjusted(0, 102, 0, -2), Qt.AlignmentFlag.AlignCenter,
                   f"{self.gear if self.gear else '—'}  ·  {self.speed:.0f} km/h")


class HudWindow(QWidget):
    def __init__(self) -> None:
        super().__init__(None, Qt.WindowType.Tool | Qt.WindowType.FramelessWindowHint |
                         Qt.WindowType.WindowStaysOnTopHint)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setWindowTitle("GearShiftHorizon HUD")
        self._base_size = (330, 255)
        self.setFixedSize(*self._base_size)
        self.setWindowOpacity(0.88)
        self._drag_offset = None
        self.tach = Tachometer()
        self.state_label = QLabel("ОЖИДАНИЕ ТЕЛЕМЕТРИИ")
        self.state_label.setStyleSheet(f"color:{GREEN};font-weight:700")
        self.detail = QLabel("Подключи Data Out в Forza · UDP 5555")
        self.detail.setWordWrap(True)
        self.detail.setStyleSheet(f"color:{MUTED}")
        self.reason = QLabel("TCU: анализ не начат")
        self.reason.setWordWrap(True)
        self.reason.setStyleSheet(f"color:{TEXT}")
        layout = QVBoxLayout(self)
        layout.setContentsMargins(14, 12, 14, 12)
        layout.setSpacing(4)
        layout.addWidget(self.state_label)
        layout.addWidget(self.tach, 1)
        layout.addWidget(self.detail)
        layout.addWidget(self.reason)
        self.setStyleSheet("background:rgba(16,23,34,224);border:1px solid #40536b;border-radius:14px;")

    def set_scale(self, percent: int) -> None:
        scale = max(70, min(140, int(percent))) / 100.0
        self.setFixedSize(int(self._base_size[0] * scale), int(self._base_size[1] * scale))

    def set_opacity(self, percent: int) -> None:
        self.setWindowOpacity(max(35, min(100, int(percent))) / 100.0)

    def update_data(self, frame, analysis, decision) -> None:
        self.tach.set_values(frame)
        if analysis:
            self.state_label.setText(f"{analysis.state.value}  ·  {analysis.confidence:.0%}")
            self.detail.setText(
                f"Газ {analysis.throttle:.0%}  Тормоз {analysis.brake:.0%}  "
                f"Slip {analysis.max_wheel_slip:.2f}  Lat {analysis.lateral_accel_raw:+.2f} raw  "
                f"Steer {analysis.steering_input:+.0%}  "
                f"{frame.power_w / 1000:.0f} kW · {frame.torque_nm:.0f} Nm  "
                f"Boost {frame.boost_raw:.1f} PSI"
            )
        else:
            self.state_label.setText("НЕТ ТЕЛЕМЕТРИИ")
            self.detail.setText("Сигнал FH6 потерян · автоматическое управление приостановлено")
            self.reason.setText("TCU: ожидание данных")
        if decision:
            target = f"→ {decision.requested_gear}" if decision.requested_gear else "HOLD"
            predicted = f" · прогноз {decision.predicted_rpm:.0f} RPM" if decision.predicted_rpm else ""
            self.reason.setText(f"TCU: {target} · {decision.reason}{predicted}")

    def mousePressEvent(self, event) -> None:
        if event.button() == Qt.MouseButton.LeftButton:
            self._drag_offset = event.globalPosition().toPoint() - self.frameGeometry().topLeft()

    def mouseMoveEvent(self, event) -> None:
        if self._drag_offset is not None and event.buttons() & Qt.MouseButton.LeftButton:
            self.move(event.globalPosition().toPoint() - self._drag_offset)

    def mouseReleaseEvent(self, event) -> None:
        del event
        self._drag_offset = None


class MainWindow(QMainWindow):
    hud_toggled = Signal(bool)
    automatic_toggled = Signal(bool)
    hold_changed = Signal(int)
    hud_scale_changed = Signal(int)
    hud_opacity_changed = Signal(int)

    def __init__(self) -> None:
        super().__init__()
        self.setWindowTitle("GearShiftHorizon 0.20.0 · Python Edition")
        self.resize(1280, 820)
        self.setMinimumSize(980, 650)
        self.setStyleSheet(f"""
            QMainWindow,QWidget {{ background:{DARK}; color:{TEXT}; font-family:'Segoe UI'; }}
            QFrame#panel {{ background:{PANEL}; border:1px solid #2b3a4e; border-radius:12px; }}
            QLabel#muted {{ color:{MUTED}; }}
            QPushButton {{ background:#197fb9; color:white; border:0; border-radius:8px;
                          padding:10px 16px; font-weight:600; }}
            QPushButton:checked {{ background:#176b53; }}
            QCheckBox {{ spacing:8px; padding:5px; }}
            QSlider::groove:horizontal {{ height:6px; background:#34465d; border-radius:3px; }}
            QSlider::handle:horizontal {{ background:{ACCENT}; width:16px; margin:-5px 0; border-radius:8px; }}
        """)
        root = QWidget()
        self.setCentralWidget(root)
        outer = QVBoxLayout(root)
        outer.setContentsMargins(24, 20, 24, 20)
        outer.setSpacing(16)

        header = QHBoxLayout()
        title = QLabel("GEARSHIFT HORIZON")
        title.setStyleSheet("font-size:22px;font-weight:800;letter-spacing:1px")
        version = QLabel("PYTHON EDITION  ·  0.20.0")
        version.setObjectName("muted")
        header.addWidget(title)
        header.addStretch()
        header.addWidget(version)
        outer.addLayout(header)
        self.car_label = QLabel("Автомобиль: ожидание телеметрии")
        self.car_label.setObjectName("muted")
        outer.addWidget(self.car_label)

        cards = QGridLayout()
        cards.setSpacing(14)
        self.metric_labels = {}
        for index, (key, caption) in enumerate([
            ("speed", "СКОРОСТЬ"), ("rpm", "ОБОРОТЫ"), ("gear", "ПЕРЕДАЧА"),
            ("state", "СОСТОЯНИЕ"),
        ]):
            card = QFrame()
            card.setObjectName("panel")
            box = QVBoxLayout(card)
            name = QLabel(caption)
            name.setObjectName("muted")
            value = QLabel("—")
            value.setStyleSheet("font-size:25px;font-weight:750")
            box.addWidget(name)
            box.addWidget(value)
            self.metric_labels[key] = value
            cards.addWidget(card, 0, index)
        outer.addLayout(cards)

        content = QGridLayout()
        content.setSpacing(14)
        left = QFrame()
        left.setObjectName("panel")
        left_box = QVBoxLayout(left)
        left_box.addWidget(QLabel("LIVE TACHOMETER"))
        self.tach = Tachometer()
        left_box.addWidget(self.tach, 1)
        self.analysis_label = QLabel("Ожидание телеметрии")
        self.analysis_label.setWordWrap(True)
        self.analysis_label.setStyleSheet(f"color:{MUTED}")
        left_box.addWidget(self.analysis_label)
        content.addWidget(left, 0, 0)

        right = QFrame()
        right.setObjectName("panel")
        right_box = QVBoxLayout(right)
        right_box.addWidget(QLabel("КОНТРОЛЛЕР И ДИАГНОСТИКА"))
        self.decision_label = QLabel("Переключения отключены до проверки калибровки.")
        self.decision_label.setWordWrap(True)
        right_box.addWidget(self.decision_label)
        self.auto_check = QCheckBox("Разрешить автоматические переключения")
        self.auto_check.setChecked(False)
        self.auto_check.toggled.connect(self.automatic_toggled.emit)
        right_box.addWidget(self.auto_check)
        self.hud_button = QPushButton("HUD: ВЫКЛ")
        self.hud_button.setCheckable(True)
        self.hud_button.toggled.connect(self._hud_changed)
        right_box.addWidget(self.hud_button)
        right_box.addWidget(QLabel("Длительность импульса клавиши"))
        self.hold_slider = QSlider(Qt.Orientation.Horizontal)
        self.hold_slider.setRange(10, 120)
        self.hold_slider.setValue(40)
        self.hold_slider.valueChanged.connect(self.hold_changed.emit)
        right_box.addWidget(self.hold_slider)
        self.hold_value = QLabel("40 мс")
        right_box.addWidget(self.hold_value)
        self.hold_slider.valueChanged.connect(lambda v: self.hold_value.setText(f"{v} мс"))
        right_box.addWidget(QLabel("Размер игрового HUD"))
        self.hud_scale = QSlider(Qt.Orientation.Horizontal)
        self.hud_scale.setRange(70, 140)
        self.hud_scale.setValue(100)
        self.hud_scale.valueChanged.connect(self.hud_scale_changed.emit)
        right_box.addWidget(self.hud_scale)
        right_box.addWidget(QLabel("Прозрачность HUD"))
        self.hud_opacity = QSlider(Qt.Orientation.Horizontal)
        self.hud_opacity.setRange(35, 100)
        self.hud_opacity.setValue(88)
        self.hud_opacity.valueChanged.connect(self.hud_opacity_changed.emit)
        right_box.addWidget(self.hud_opacity)
        self.connection_label = QLabel("UDP 5555 · ожидание Forza Data Out")
        self.connection_label.setObjectName("muted")
        right_box.addStretch()
        right_box.addWidget(self.connection_label)
        content.addWidget(right, 0, 1)
        content.setColumnStretch(0, 1)
        content.setColumnStretch(1, 1)
        outer.addLayout(content, 1)

        footer = QLabel("Телеметрия анализируется локально · профиль автомобиля сохраняется отдельно")
        footer.setObjectName("muted")
        outer.addWidget(footer)

    def _hud_changed(self, enabled: bool) -> None:
        self.hud_button.setText("HUD: ВКЛ" if enabled else "HUD: ВЫКЛ")
        self.hud_toggled.emit(enabled)

    def update_snapshot(self, frame, analysis, decision, connected: bool, profile=None) -> None:
        if frame:
            self.car_label.setText(
                f"Автомобиль ID {profile.car_ordinal} · класс {frame.car_class_label} "
                f"(Data Out {profile.car_class}) · PI {profile.pi} · привод {profile.drivetrain} · "
                f"обучено передач: {sum(1 for n in profile.gear_samples.values() if n >= 30)}"
                if profile else f"Автомобиль ID {frame.car_ordinal} · класс {frame.car_class_label} · PI {frame.pi}"
            )
            self.metric_labels["speed"].setText(f"{frame.speed_kmh:.0f} km/h")
            self.metric_labels["rpm"].setText(f"{frame.rpm:,.0f}")
            self.metric_labels["gear"].setText(str(frame.gear) if frame.gear else "—")
            self.metric_labels["state"].setText(analysis.state.value if analysis else "—")
            self.tach.set_values(frame)
            if analysis:
                accel = analysis.longitudinal_accel_mps2
                accel_text = f"{accel:.2f}" if accel is not None else "—"
                self.analysis_label.setText(
                    f"{analysis.reason}\nПродольное ускорение: {accel_text} m/s²"
                    f" · боковая нагрузка: {analysis.lateral_g:+.2f} G"
                    f"\nГаз {analysis.throttle:.0%} · тормоз {analysis.brake:.0%}"
                    f" · сцепление {frame.clutch_raw / 255:.0%} · ручник {frame.handbrake_raw / 255:.0%}"
                    f"\nSlip ratio {frame.max_abs_slip:.3f} · combined slip {frame.max_abs_combined_slip:.3f}"
                    f" · руль {analysis.steering_input:+.0%}"
                    f"\nCollision Δv {frame.smashable_vel_diff:.2f} m/s · масса {frame.smashable_mass:.1f} kg"
                    f"\nBoost {frame.boost_raw:.1f} PSI · топливо {frame.fuel:.0%}"
                    f" · температура шин {sum(frame.tire_temps) / 4:.1f} (raw)"
                )
            if decision:
                predicted = f"\nПрогноз: {decision.predicted_rpm:.0f} RPM" if decision.predicted_rpm else ""
                self.decision_label.setText(f"{decision.direction}: {decision.reason}{predicted}")
        else:
            for value in self.metric_labels.values():
                value.setText("—")
            self.car_label.setText("Автомобиль: ожидание телеметрии")
            self.tach.set_values(None)
            self.analysis_label.setText("Телеметрия отсутствует")
            self.decision_label.setText("TCU приостановлен до восстановления сигнала")
        self.connection_label.setText("UDP подключён · получаем телеметрию" if connected
                                      else "Нет телеметрии · проверь Data Out / UDP 5555")
