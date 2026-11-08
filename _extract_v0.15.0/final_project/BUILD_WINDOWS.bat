@echo off
setlocal
cd /d "%~dp0"
echo Building FH6 Virtual TCU...
cargo build --release
if errorlevel 1 (
  echo.
  echo BUILD FAILED. Check the error above.
  pause
  exit /b 1
)
if not exist "target\release\fh6-virtual-tcu-rust.exe" (
  echo EXE was not found in target\release.
  pause
  exit /b 1
)
if not exist "dist\data" mkdir "dist\data"
if exist "tcu_config.json" if not exist "dist\data\tcu_config.json" copy /Y "tcu_config.json" "dist\data\tcu_config.json" >nul
if exist "tcu_calibration.json" if not exist "dist\data\tcu_calibration.json" copy /Y "tcu_calibration.json" "dist\data\tcu_calibration.json" >nul
copy /Y "target\release\fh6-virtual-tcu-rust.exe" "dist\FH6-Virtual-TCU.exe" >nul
echo.
echo DONE: dist\FH6-Virtual-TCU.exe
echo Keep the data folder beside the EXE. Settings and calibration are saved there.
pause
