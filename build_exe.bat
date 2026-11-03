@echo off
setlocal
cd /d "%~dp0"

echo ===================================================
echo   Building GloWled Standalone Portable Executable
echo ===================================================
echo.

python -m pip install --upgrade pyinstaller -r requirements.txt
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Failed to install requirements or pyinstaller.
    pause
    exit /b %ERRORLEVEL%
)

echo Building GloWled.exe...
pyinstaller --clean --noconsole --onefile --icon=icon.ico --add-data "icon.ico;." --name=GloWled main.py
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] PyInstaller compilation failed.
    pause
    exit /b %ERRORLEVEL%
)

echo.
echo ===================================================
echo [SUCCESS] Build finished successfully!
echo Executable ready at: dist\GloWled.exe
echo ===================================================
pause
