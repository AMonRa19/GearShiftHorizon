# FH6 Virtual TCU — Rust port (foundation)

Это отдельная Rust-реализация, не модифицирующая оригинальный репозиторий.
Цель — перенос функциональности Virtual TCU 13.2.8 с сохранением протокола
телеметрии FH6 и клавиатурного управления E/Q.

## Что уже включено
- Парсер FH6 UDP-пакета (324 байта; поля по смещениям исходного parser.py).
- UDP-приёмник на порту 5555.
- Отдельный поток клавиатурного вывода с последовательным press/release.
- Базовый автоматический TCU: brake downshift, kickdown, torque recovery, upshift.
- Сброс временного состояния при смене сессии и тайм-ауте телеметрии.
- Простая GUI-панель egui и сохранение настроек в `tcu_config.json`.

## Важно
Это **первый переносимый фундамент, а не завершённый feature-for-feature клон 13.2.8**.
В исходнике есть большие подсистемы, которые ещё предстоит перенести: точная логика
Comfort/Race/Drift/Off-road, калибровка передаточных отношений, power curve,
профили автомобилей и тюнинга, adaptive drive style, reverse-hold, wheelspin,
landing recovery, web dashboard/WebSocket, replay/logger и полный набор настроек.
Их нельзя честно объявить реализованными в этом стартовом пакете.

## Сборка (Windows)
1. Установить Rust stable через rustup.
2. В папке проекта выполнить:
   `cargo build --release`
3. Запустить:
   `cargo run --release`

Для отправки клавиш приложению может потребоваться запуск с правами,
которые позволяют Windows принимать синтетический ввод. Сначала тестируйте
на безопасной площадке и проверьте назначения E/Q в FH6.

## Настройка FH6
Включить Data Out / телеметрию UDP, адрес `127.0.0.1`, порт `5555`.
Убедиться, что клавиши E и Q назначены на повышение и понижение передач.

## Тестирование
`cargo test`

## Известные ограничения текущего этапа
- Отправка клавиш через rdev должна быть проверена на целевой Windows-сборке.
- В этой версии выбран упрощённый набор правил, не копирующий все параметры и
  защиты исходной логики; не использовать как готовую гоночную автоматику.
- Временная блокировка shift проверяется по факту обработки пакетов, а не по
  UI-истории; дальнейший этап добавит подтверждение фактического изменения gear
  и повторную отправку только при доказанном отсутствии реакции игры.

## Browser dashboard (new)

The Rust backend now starts a local settings page at `http://127.0.0.1:8765` alongside the desktop egui window. Both interfaces edit the same `tcu_config.json` configuration. The browser page is local-only and does not expose the controller to the LAN.

The Features section includes all 11 requested switches. In this early port, the safety guards for Cornering Lock, Airtime Lock and Transient Lock, plus initial Brake Curve / Turbo Lag / Drivetrain Aware adjustments are connected to the shift decision path. The remaining switches are configuration/UI placeholders until their corresponding learning and decision algorithms are ported and tested; they are not yet feature-equivalent to Virtual TCU 13.2.8.
