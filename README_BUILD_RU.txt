СБОРКА FH6 VIRTUAL TCU ДЛЯ WINDOWS

1. Установите Rust с https://rustup.rs/.
2. Если сборка сообщит об отсутствии linker/MSVC, установите Visual Studio Build Tools с компонентом Desktop development with C++.
3. Запустите BUILD_WINDOWS.bat двойным щелчком.
4. Готовая программа появится в dist\FH6-Virtual-TCU.exe.

Рядом с EXE создаётся папка data. В ней автоматически хранятся:
- tcu_config.json — настройки;
- tcu_calibration.json — профили автомобилей и обученные данные.

Для переноса на другой ПК копируйте FH6-Virtual-TCU.exe и папку data вместе.
Папка target — временные файлы компиляции, для запуска не нужна.
В Release-сборке консольное окно скрыто. Зависимости Rust компонуются в EXE; отдельные DLL от Rust-крейтов обычно не требуются.
