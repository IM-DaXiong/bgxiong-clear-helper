@echo off
setlocal EnableExtensions EnableDelayedExpansion

title bgxiong-clear-helper - One-Click Package

cd /d "%~dp0"

echo ========================================
echo   BGXiong Clear Helper - Package
echo ========================================
echo.

REM ---- Check cargo ----
where cargo >nul 2>&1
if errorlevel 1 (
    echo [ERROR] cargo not found. Install Rust from https://rustup.rs/
    exit /b 1
)

REM ---- Read version from Cargo.toml ----
set "APP_VERSION=0.0.0"
for /f "usebackq tokens=1,2 delims==" %%A in (`findstr /b /c:"version" Cargo.toml`) do (
    set "RAW=%%B"
    goto :got_ver
)
:got_ver
set "RAW=!RAW: =!"
set "RAW=!RAW:"=!"
if not "!RAW!"=="" set "APP_VERSION=!RAW!"

set "APP_NAME=bgxiong-clear-helper"
set "EXE_NAME=%APP_NAME%.exe"
set "PKG_NAME=%APP_NAME%-v%APP_VERSION%-windows-x64"
set "SRC=target\release\%EXE_NAME%"
set "DIST_DIR=dist"
set "STAGE_DIR=%DIST_DIR%\%PKG_NAME%"
set "ZIP_PATH=%DIST_DIR%\%PKG_NAME%.zip"

echo Version: %APP_VERSION%
echo.

REM ---- Build ----
echo [1/4] cargo build --release
echo.
cargo build --release
if errorlevel 1 (
    echo.
    echo [ERROR] Build failed.
    exit /b 1
)

if not exist "%SRC%" (
    echo [ERROR] Output not found: %SRC%
    exit /b 1
)

REM ---- Stage package folder ----
echo.
echo [2/4] Staging package files...

if not exist "%DIST_DIR%" mkdir "%DIST_DIR%"
if exist "%STAGE_DIR%" rmdir /s /q "%STAGE_DIR%"
mkdir "%STAGE_DIR%"

copy /Y "%SRC%" "%STAGE_DIR%\%EXE_NAME%" >nul
if errorlevel 1 (
    echo [ERROR] Copy exe failed. Close the running app and retry.
    exit /b 1
)

if exist "README.md" copy /Y "README.md" "%STAGE_DIR%\README.md" >nul
if exist "docs\feature-plan-browser-appdata-registry.md" (
    mkdir "%STAGE_DIR%\docs" >nul 2>&1
    copy /Y "docs\feature-plan-browser-appdata-registry.md" "%STAGE_DIR%\docs\" >nul
)

REM ---- Flat copy for quick run ----
copy /Y "%SRC%" "%DIST_DIR%\%EXE_NAME%" >nul

REM ---- Zip ----
echo.
echo [3/4] Creating zip: %ZIP_PATH%

if exist "%ZIP_PATH%" del /f /q "%ZIP_PATH%"

powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "Compress-Archive -Path '%STAGE_DIR%\*' -DestinationPath '%ZIP_PATH%' -Force"
if errorlevel 1 (
    echo [ERROR] Zip creation failed.
    exit /b 1
)

if not exist "%ZIP_PATH%" (
    echo [ERROR] Zip not found after Compress-Archive.
    exit /b 1
)

REM ---- Summary ----
echo.
echo [4/4] Package ready
echo.
echo   EXE:  %CD%\%DIST_DIR%\%EXE_NAME%
for %%A in ("%DIST_DIR%\%EXE_NAME%") do echo         %%~zA bytes
echo   DIR:  %CD%\%STAGE_DIR%
echo   ZIP:  %CD%\%ZIP_PATH%
for %%A in ("%ZIP_PATH%") do echo         %%~zA bytes
echo.
echo Done.
exit /b 0
