# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2025-present Iron Curtain contributors

# ci-local.ps1 - Local CI validation for cnc-formats (PowerShell)
# Run all CI checks locally before pushing.
# Mirrors the GitHub Actions workflow in .github/workflows/ci.yml.

$ErrorActionPreference = "Stop"

Write-Host "=== cnc-formats - Local CI ===" -ForegroundColor Cyan

# -- Locate cargo ---------------------------------------------------------
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoPaths = @(
        "$env:USERPROFILE\.cargo\bin\cargo.exe",
        "$env:HOME\.cargo\bin\cargo.exe",
        "$env:HOME\.cargo\bin\cargo",
        "C:\Users\$env:USERNAME\.cargo\bin\cargo.exe"
    )

    $cargoFound = $false
    foreach ($cargoPath in $cargoPaths) {
        if (Test-Path $cargoPath) {
            $env:PATH = (Split-Path $cargoPath) + ";" + $env:PATH
            Write-Host "* Found cargo at: $cargoPath" -ForegroundColor Green
            $cargoFound = $true
            break
        }
    }

    if (-not $cargoFound -and -not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "ERROR: cargo not found. Install Rust from https://rustup.rs/" -ForegroundColor Red
        exit 1
    }
}

Write-Host "* Using cargo: $(Get-Command cargo | Select-Object -ExpandProperty Source)" -ForegroundColor Green

# -- Rust version info -----------------------------------------------------
$rustVersion = & rustc --version
Write-Host "Rust version: $rustVersion" -ForegroundColor Magenta

if ($rustVersion -match "nightly") {
    Write-Host "WARNING: You are using nightly Rust, but GitHub Actions uses stable!" -ForegroundColor Yellow
    Write-Host "   Consider testing with: rustup default stable" -ForegroundColor Yellow
}
Write-Host ""

# -- Helpers ---------------------------------------------------------------

function Run-Check {
    param(
        [string]$Name,
        [string]$Command
    )

    Write-Host "Running: $Name" -ForegroundColor Blue
    Write-Host "Command: $Command" -ForegroundColor Gray

    $startTime = Get-Date
    $oldErrorAction = $ErrorActionPreference
    $ErrorActionPreference = "Continue"

    try {
        Invoke-Expression "$Command 2>&1" | ForEach-Object { Write-Host $_ }
        $exitCode = $LASTEXITCODE
        $ErrorActionPreference = $oldErrorAction

        if ($exitCode -ne 0) {
            throw "Command failed with exit code $exitCode"
        }
        $endTime = Get-Date
        $duration = ($endTime - $startTime).TotalSeconds
        Write-Host "PASS: $Name ($([math]::Round($duration))s)" -ForegroundColor Green
        Write-Host ""
    } catch {
        $ErrorActionPreference = $oldErrorAction
        $endTime = Get-Date
        $duration = ($endTime - $startTime).TotalSeconds
        Write-Host "FAIL: $Name ($([math]::Round($duration))s)" -ForegroundColor Red
        Write-Host "ERROR: Fix the issue above before pushing." -ForegroundColor Red
        exit 1
    }
}

function Run-Fix {
    param(
        [string]$Name,
        [string]$Command
    )

    Write-Host "Auto-fixing: $Name" -ForegroundColor Blue
    Write-Host "Command: $Command" -ForegroundColor Gray

    $startTime = Get-Date
    $oldErrorAction = $ErrorActionPreference
    $ErrorActionPreference = "Continue"

    try {
        Invoke-Expression "$Command 2>&1" | ForEach-Object { Write-Host $_ }
        $exitCode = $LASTEXITCODE
        $ErrorActionPreference = $oldErrorAction

        if ($exitCode -ne 0) {
            throw "Command failed with exit code $exitCode"
        }
        $endTime = Get-Date
        $duration = ($endTime - $startTime).TotalSeconds
        Write-Host "DONE: $Name ($([math]::Round($duration))s)" -ForegroundColor Green
        Write-Host ""
    } catch {
        $ErrorActionPreference = $oldErrorAction
        $endTime = Get-Date
        $duration = ($endTime - $startTime).TotalSeconds
        Write-Host "WARNING: $Name failed ($([math]::Round($duration))s) -- continuing" -ForegroundColor Yellow
        Write-Host ""
    }
}

# -- Pre-flight: project root ----------------------------------------------
if (-not (Test-Path "Cargo.toml")) {
    Write-Host "ERROR: Cargo.toml not found. Run this from the project root." -ForegroundColor Red
    exit 1
}

# -- UTF-8 encoding validation ---------------------------------------------
Write-Host "Validating UTF-8 encoding..." -ForegroundColor Cyan

function Test-Utf8Encoding {
    param([string]$FilePath)

    if (-not (Test-Path $FilePath)) {
        Write-Host "ERROR: File not found: $FilePath" -ForegroundColor Red
        return $false
    }

    try {
        $null = Get-Content $FilePath -Encoding UTF8 -ErrorAction Stop
        $bytes = [System.IO.File]::ReadAllBytes($FilePath)
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            Write-Host "ERROR: $FilePath has UTF-8 BOM (remove it)" -ForegroundColor Red
            return $false
        }
        Write-Host "  OK: $FilePath" -ForegroundColor Green
        return $true
    } catch {
        Write-Host "ERROR: $FilePath is not valid UTF-8" -ForegroundColor Red
        return $false
    }
}

if (-not (Test-Utf8Encoding "README.md")) { exit 1 }
if (-not (Test-Utf8Encoding "Cargo.toml")) { exit 1 }

$rustFiles = Get-ChildItem -Path "src" -Filter "*.rs" -Recurse
foreach ($file in $rustFiles) {
    if (-not (Test-Utf8Encoding $file.FullName)) { exit 1 }
}
Write-Host ""

# -- Auto-fix --------------------------------------------------------------
Write-Host "Auto-fixing formatting and lint..." -ForegroundColor Cyan
Run-Fix "Format" "cargo fmt"
Run-Fix "Clippy auto-fix" "cargo clippy --fix --allow-dirty --allow-staged --all-targets --all-features"
Run-Fix "Format (post-clippy)" "cargo fmt"

Write-Host "Running CI checks..." -ForegroundColor Cyan
Write-Host ""

# -- 1. Format check -------------------------------------------------------
Run-Check "Format check" "cargo fmt --check"

# -- 2. Clippy (all features) ----------------------------------------------
Run-Check "Clippy (all features)" "cargo clippy --tests --all-features -- -D warnings"

# -- 3. Clippy (no default features -- without blowfish) --------------------
Run-Check "Clippy (no default features)" "cargo clippy --tests --no-default-features -- -D warnings"

# -- 4. Tests (all features) -----------------------------------------------
Run-Check "Tests (all features)" "cargo test --all-features"

# -- 5. Tests (no default features) ----------------------------------------
Run-Check "Tests (no default features)" "cargo test --no-default-features"

# -- 6. Documentation ------------------------------------------------------
$env:RUSTDOCFLAGS = "-D warnings"
Run-Check "Documentation" "cargo doc --no-deps --document-private-items --all-features"

# -- 7. License check (cargo-deny) -----------------------------------------
Write-Host "Running license check..." -ForegroundColor Cyan
if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
    Run-Check "License check (cargo deny)" "cargo deny check licenses"
} else {
    Write-Host "WARNING: cargo-deny not found. Installing..." -ForegroundColor Yellow
    try {
        & cargo install cargo-deny --locked
        if ($LASTEXITCODE -eq 0) {
            Run-Check "License check (cargo deny)" "cargo deny check licenses"
        } else { throw "install failed" }
    } catch {
        Write-Host "WARNING: Could not install cargo-deny. Skipping license check." -ForegroundColor Yellow
        Write-Host "  Install manually: cargo install cargo-deny" -ForegroundColor Yellow
    }
}

# -- 8. Security audit (cargo-audit) ---------------------------------------
Write-Host "Running security audit..." -ForegroundColor Cyan
if (Get-Command cargo-audit -ErrorAction SilentlyContinue) {
    Run-Check "Security audit" "cargo audit"
} else {
    Write-Host "WARNING: cargo-audit not found. Installing..." -ForegroundColor Yellow
    try {
        & cargo install cargo-audit --locked
        if ($LASTEXITCODE -eq 0) {
            Run-Check "Security audit" "cargo audit"
        } else { throw "install failed" }
    } catch {
        Write-Host "WARNING: Could not install cargo-audit. Skipping security audit." -ForegroundColor Yellow
        Write-Host "  Install manually: cargo install cargo-audit" -ForegroundColor Yellow
    }
}

# -- 9. MSRV check (rust-version from Cargo.toml) --------------------------
$msrv = "1.85"
Write-Host "Checking MSRV ($msrv)..." -ForegroundColor Cyan
if (Get-Command rustup -ErrorAction SilentlyContinue) {
    $toolchains = & rustup toolchain list
    $hasMsrv = $toolchains -match [regex]::Escape($msrv)

    if (-not $hasMsrv) {
        Write-Host "Installing Rust $msrv toolchain..." -ForegroundColor Yellow
        & rustup toolchain install $msrv --profile minimal
        if ($LASTEXITCODE -ne 0) {
            Write-Host "WARNING: Could not install Rust $msrv. Skipping MSRV check." -ForegroundColor Yellow
            $hasMsrv = $false
        } else {
            $hasMsrv = $true
        }
    }

    if ($hasMsrv) {
        # Ensure clippy is available for MSRV
        $clippy = & rustup component list --toolchain $msrv 2>$null | Select-String "clippy.*(installed)"
        if (-not $clippy) {
            & rustup component add clippy --toolchain $msrv
        }

        $env:CARGO_TARGET_DIR = "target/msrv"
        Run-Check "MSRV compile (Rust $msrv)" "rustup run $msrv cargo check --all-targets --all-features"
        Run-Check "MSRV clippy (Rust $msrv)" "rustup run $msrv cargo clippy --tests --all-features -- -D warnings"
        Run-Check "MSRV test (Rust $msrv)" "rustup run $msrv cargo test --all-features"
        $env:CARGO_TARGET_DIR = $null
    }
} else {
    Write-Host "WARNING: rustup not found. Skipping MSRV check." -ForegroundColor Yellow
}

# -- Done ------------------------------------------------------------------
Write-Host ""
Write-Host "All CI checks passed!" -ForegroundColor Green
Write-Host "Review any auto-fixes, then push." -ForegroundColor Blue
