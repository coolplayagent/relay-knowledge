@echo off
chcp 65001 >nul
setlocal EnableExtensions

echo Checking Rust toolchain...
where rustup >nul 2>&1
if %errorlevel% neq 0 (
    echo rustup not found, installing Rust toolchain...
    where winget >nul 2>&1
    if %errorlevel% neq 0 (
        echo [Error] rustup not found. Install Rust from https://rustup.rs/ and rerun this script.
        exit /b 1
    )
    winget install --id Rustlang.Rustup -e --source winget
    if %errorlevel% neq 0 (
        echo [Error] Rust toolchain installation failed.
        exit /b 1
    )
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)

rustup toolchain install stable --profile minimal --component rustfmt --component clippy
if %errorlevel% neq 0 (
    echo [Error] Rust toolchain setup failed.
    exit /b 1
)

rustup component add rustfmt clippy
if %errorlevel% neq 0 (
    echo [Error] Rust component setup failed.
    exit /b 1
)

where cargo >nul 2>&1
if %errorlevel% neq 0 (
    echo [Error] cargo not found. Ensure Rust is installed and %%USERPROFILE%%\.cargo\bin is on PATH.
    exit /b 1
)

echo Checking pre-commit...
set "PYTHON_CMD="
py -3 --version >nul 2>&1
if %errorlevel% equ 0 set "PYTHON_CMD=py -3"
if "%PYTHON_CMD%"=="" python --version >nul 2>&1
if %errorlevel% equ 0 if "%PYTHON_CMD%"=="" set "PYTHON_CMD=python"

set "PRE_COMMIT_CMD="
pre-commit --version >nul 2>&1
if %errorlevel% equ 0 set "PRE_COMMIT_CMD=pre-commit"
if "%PRE_COMMIT_CMD%"=="" if not "%PYTHON_CMD%"=="" %PYTHON_CMD% -m pre_commit --version >nul 2>&1
if %errorlevel% equ 0 if "%PRE_COMMIT_CMD%"=="" if not "%PYTHON_CMD%"=="" set "PRE_COMMIT_CMD=%PYTHON_CMD% -m pre_commit"
if "%PRE_COMMIT_CMD%"=="" if not "%PYTHON_CMD%"=="" (
    echo pre-commit not found, installing pre-commit...
    %PYTHON_CMD% -m pip install --user pre-commit
    if %errorlevel% equ 0 set "PRE_COMMIT_CMD=%PYTHON_CMD% -m pre_commit"
)

if not "%PRE_COMMIT_CMD%"=="" (
    echo Installing git hooks...
    %PRE_COMMIT_CMD% install
    if %errorlevel% neq 0 (
        echo [Warning] Git hooks install failed.
    ) else (
        echo Git hooks install successful.
    )
) else (
    echo [Warning] Python or pre-commit not found. Skipping git hook installation.
)

where npm >nul 2>&1
if %errorlevel% equ 0 (
    echo npm found.
) else (
    echo [Warning] npm not found. Install Node.js/npm before building Web assets.
)

where uv >nul 2>&1
if %errorlevel% equ 0 (
    echo uv found.
) else (
    echo [Warning] uv not found. Browser integration tests will be skipped.
)

echo Environment setup completed. Use build.sh to build and check.sh to verify on Unix-like systems.
