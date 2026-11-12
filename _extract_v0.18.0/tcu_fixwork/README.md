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
Это **промежуточная Rust-реализация, а не завершённый feature-for-feature клон 13.2.8**.
Уже добавлены базовый web dashboard, отдельные профили автокалибровки по ID машины, сбор кривой мощности, оценка отношений скорость/RPM и графики. Полный перенос исходных алгоритмов Comfort/Race/Drift/Off-road, reverse-hold, wheelspin, landing recovery, replay/logger и WebSocket ещё впереди. Не считать текущие эвристики окончательно проверенными без тестов в FH6.

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

## Автокалибровка по автомобилям

Калибратор хранит отдельный профиль для каждого `car_ordinal` FH6 в `tcu_calibration.json`. При смене автомобиля активный профиль переключается автоматически. Система собирает:
- усреднённую зависимость скорости от RPM на каждой передаче (оценка передаточного коэффициента);
- рабочую кривую мощности по диапазонам RPM при высокой нагрузке;
- фактические RPM-пороги переключений, наблюдаемые в телеметрии, и постепенно адаптирует пороги автоматики после накопления минимум трёх наблюдений.

В desktop UI и браузере отображаются текущий автомобиль, объём выборки, обученные передачи, пороги и графики RPM/мощности. В браузере данные доступны через `GET /api/state`. Кнопка сброса очищает только профиль активного автомобиля.

Важно: это начальная версия онлайн-обучения. FH6 UDP telemetry не сообщает достоверный VIN/название автомобиля, поэтому ключ профиля — `car_ordinal`. События смены передачи сейчас выводятся из последовательных телеметрических пакетов; отдельного подтверждения источника переключения (водитель или TCU) пока нет. Не считать пороги окончательно откалиброванными без проверки на нескольких машинах и сравнения логов.


### Исправление торможения двигателем

Автоматическое понижение при торможении теперь использует обученные передаточные коэффициенты конкретной машины: оценивает обороты после перехода на следующую нижнюю передачу и переключает только в безопасном диапазоне. При сильном торможении допускается более высокий целевой RPM, но с запасом до ограничителя. Пока для пары машина/передача нет минимум 8 измерений, используется консервативный резервный порог. Для тормозных понижений интервал между командами ограничен сверху 180 мс (при этом фактическая скорость зависит также от задержки клавиатурной команды и реакции игры). Функция активна, когда включены Brake Downshift и Engine Brake.


## Shift confirmation and faster per-car learning
- The TCU now waits for telemetry to confirm each requested gear before sending another shift command (up to 1.4 s). This prevents queued/cascading upshifts through several gears.
- Per-gear ratio learning uses clean, stable-gear samples and a faster exponential moving average.
- Learned shift thresholds are applied after two observed manual shifts and adapt more quickly, with safety clamps.
- If FH6 does not confirm a shift within 1.4 s, the pending lock expires; the TCU can retry after the normal shift interval.

## Адаптация переключений и торможение двигателем (обновление)
- Калибровка передаточного отношения начинает использовать профиль после 3 чистых образцов вместо 4; EMA быстрее адаптируется к машине.
- Кривая мощности обучается уже при уверенном газе от 55%.
- Пороговые наблюдения учитывают тормозные понижения начиная с 10% педали тормоза.
- При торможении целевые обороты после понижения рассчитываются по выученному отношению передачи; пока данных мало, используется консервативная оценка одного шага.
- Автоматические переключения используют общий cooldown 350 мс для повышения и понижения; каждое следующее переключение ожидает подтверждения фактической передачи по телеметрии.
- Повторное повышение дополнительно защищено защёлкой `upshift_armed`, чтобы исключить каскад 7→8→9→10.

Калибровка остаётся оценочной: телеметрия FH6 не предоставляет прямые параметры коробки из меню тюнинга. Проверяйте новые пороги на безопасном участке и следите за прогнозируемыми оборотами после понижения.

## Масштабируемый интерфейс и горизонтальная компоновка

- Desktop UI открывается в окне 1500×920 с минимальным размером 980×650.
- В desktop-интерфейсе есть регулятор масштаба 80–135%; основные блоки расположены горизонтально, а графики показываются рядом.
- В браузерной панели используется доступная ширина окна, трёхколоночная компоновка на широких экранах, две колонки на средних и одна на узких. Масштаб браузерной панели регулируется отдельно и сохраняется в localStorage.


## Профили Drag/Drift и задержка переключений

- Добавлен профиль `Drag`: повышает порог апшифта (удержание передачи до высоких оборотов), снижает склонность к лишним дауншифтам и использует более спокойный интервал команд.
- Профиль `Drift`: уменьшает задержку реакции, раньше запрашивает понижение при нажатии газа и ослабляет Cornering/Transient Lock только в пределах более высокого порога, чтобы контролируемый занос не блокировал каждую команду. Защита от чрезмерной боковой нагрузки и сильного срыва остаётся. Автоматическое торможение двигателем в Drift выключено, чтобы не провоцировать резкий перенос веса; для замедления используйте Race/Comfort.
- Интервал автоматического переключения фиксирован на 350 мс, как в оригинальном TCU; длительность импульса клавиши и пауза между импульсами задаются отдельно (`shift_hold_ms` и `shift_gap_ms`).
- `Drag` и `Drift` — эвристические пресеты. Они не считывают реальные настройки коробки из FH6; автокалибровка конкретной машины продолжает использовать `car_ordinal`. Проверяйте их на закрытой/безопасной площадке.


## Braking / kickdown correction

The current control logic uses the corrected per-gear calibration ratio (engine RPM per m/s). Older inverse-ratio samples are invalidated automatically as each gear is observed again. Braking downshifts are no longer blocked by longitudinal deceleration or the cornering lock: the cornering lock now inhibits torque-driven shifts in a loaded turn, while braking downshifts remain available on corner entry. The braking target is checked against an idle-RPM floor and a mode-dependent safe RPM ceiling, rather than requiring an unnecessarily high minimum RPM. Kickdown and torque downshift checks also use the predicted RPM in the next gear.

After updating, allow the car to collect clean steady-speed samples again. Calibration files are kept, but old inverse-ratio samples are discarded per gear when new valid samples arrive.


## Shift timing parity with upstream

Automatic shift cooldown is fixed at 350 ms, matching `Cfg.SHIFT_COOLDOWN_MS` in the upstream Forza-Love Virtual TCU. The same cooldown applies to upshifts, torque downshifts, and braking downshifts; the pending-shift acknowledgement still prevents repeated commands while the game has not confirmed a gear change. Key hold/gap timing remains separate.


### Частота обработки и импульс переключения

- Логика TCU обрабатывает каждый новый UDP-снимок сразу по приходу; при очереди выбирается самый свежий пакет с учётом session timestamp. При штатной частоте телеметрии FH6 это обычно около **60 Hz** (примерно 16,67 мс между кадрами), без дополнительного ожидания границы цикла.
- Автоматический cooldown переключений остаётся **350 мс** и не зависит от частоты цикла.
- Удержание клавиши по умолчанию — **40 мс**; пауза между повторными импульсами при переключении через несколько передач — **60 мс**. Промежуточные показания передач при многопозиционном переключении теперь считаются прогрессом одной команды, а не ошибкой подтверждения.

## Shift recovery changes (experimental, not yet compiled)

- Added a redline override at 98.5% of reported maximum RPM so normal cornering/traction locks do not leave the engine on the limiter. Pending telemetry acknowledgement still prevents repeated commands while a shift is in flight.
- Kickdown can now begin above 2 km/h rather than waiting for 18 km/h, allowing recovery when accelerating from a stop in an excessively high gear.
- Braking downshifts can begin above 4 km/h; the uncalibrated fallback no longer applies an overly high idle-RPM floor that could block every downshift at low RPM.
- Added a progressive deceleration-recovery downshift when speed falls and an impact-recovery path for sudden speed loss. Cornering/traction/airtime locks no longer suppress critical recovery paths.
- These thresholds are a first correction pass and need in-game telemetry validation across cars, especially to confirm the redline override and fallback RPM estimates do not cause over-revving.


## Adaptive learning and low-latency control

- The control worker is event-driven: it processes the newest UDP telemetry frame immediately and coalesces queued stale/out-of-order frames instead of waiting for a fixed tick boundary. At the game's normal telemetry cadence this is typically around 60 Hz.
- Per-car calibration learns RPM-per-speed ratios per gear, a filtered engine power curve, acceleration response per gear, and a bounded driving-aggression score. Samples are accepted only during stable, straight, low-slip pulls. A gear model needs at least 30 clean ratio samples; a power-curve bucket needs at least 12 samples before it can influence target-gear selection.
- Once the learned data is sufficiently populated, downshift selection evaluates candidate gears using predicted RPM and a wheel-force proxy derived from estimated engine torque and the learned ratio. Until then, conservative sequential fallback logic remains active.
- Redline and post-impact/deceleration recovery have priority over routine cornering, traction, and airtime locks; airtime lock still suppresses ordinary shifts while airborne. Ordinary shifts retain the 350 ms cooldown; critical redline protection uses its own guarded path.
- Calibration confidence is a coverage indicator based on learned ratios, power-curve points, and per-gear acceleration samples; it is not a claim of perfect or machine-learning certainty.
