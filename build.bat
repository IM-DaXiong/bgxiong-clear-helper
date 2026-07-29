@echo off
REM Compatibility entry: forward to one-click package script.
cd /d "%~dp0"
call "%~dp0package.bat"
set "ERR=%ERRORLEVEL%"
if not "%ERR%"=="0" (
    echo.
    pause
    exit /b %ERR%
)
echo.
set /p OPEN="Open dist folder? (Y/N): "
if /i "%OPEN%"=="Y" explorer "%CD%\dist"
pause
exit /b 0
