# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2025-present Iron Curtain contributors

# ci-local.ps1 - Local CI validation for cnc-formats (PowerShell)
# Run all CI checks locally before pushing.
# Mirrors the GitHub Actions workflow in .github/workflows/ci.yml.

$ErrorActionPreference = "Stop"

Write-Host "=== cnc-formats - Local CI ===" -ForegroundColor Cyan

# -- Locate Rust tools ----------------------------------------------------
function Resolve-ToolCommand {
    param(
        [string]$Name,
        [string[]]$Candidates
    )

    $command = Get-Command $Name -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($command) {
        return $command.Source
    }

    $pathSeparator = [System.IO.Path]::PathSeparator
    $pathEntries = $env:PATH -split [regex]::Escape([string]$pathSeparator)

    foreach ($candidate in $Candidates) {
        if ([string]::IsNullOrWhiteSpace($candidate) -or -not (Test-Path $candidate)) {
            continue
        }

        $toolDir = Split-Path $candidate
        if (-not ($pathEntries -contains $toolDir)) {
            $env:PATH = "$toolDir$pathSeparator$env:PATH"
            $pathEntries = $env:PATH -split [regex]::Escape([string]$pathSeparator)
        }

        return (Resolve-Path $candidate).Path
    }

    return $null
}

$cargoCandidates = @(
    "$env:USERPROFILE\.cargo\bin\cargo.exe",
    "$env:HOME\.cargo\bin\cargo.exe",
    "$env:HOME\.cargo\bin\cargo",
    "C:\Users\$env:USERNAME\.cargo\bin\cargo.exe"
)
$rustcCandidates = @(
    "$env:USERPROFILE\.cargo\bin\rustc.exe",
    "$env:HOME\.cargo\bin\rustc.exe",
    "$env:HOME\.cargo\bin\rustc",
    "C:\Users\$env:USERNAME\.cargo\bin\rustc.exe"
)
$rustupCandidates = @(
    "$env:USERPROFILE\.cargo\bin\rustup.exe",
    "$env:HOME\.cargo\bin\rustup.exe",
    "$env:HOME\.cargo\bin\rustup",
    "C:\Users\$env:USERNAME\.cargo\bin\rustup.exe"
)

$script:CargoExe = Resolve-ToolCommand "cargo" $cargoCandidates
if (-not $script:CargoExe) {
    Write-Host "ERROR: cargo not found. Install Rust from https://rustup.rs/" -ForegroundColor Red
    exit 1
}

$script:RustcExe = Resolve-ToolCommand "rustc" $rustcCandidates
if (-not $script:RustcExe) {
    Write-Host "ERROR: rustc not found next to cargo. Install Rust from https://rustup.rs/" -ForegroundColor Red
    exit 1
}

$script:RustupExe = Resolve-ToolCommand "rustup" $rustupCandidates

function global:cargo { & $script:CargoExe @args }
function global:rustc { & $script:RustcExe @args }
if ($script:RustupExe) {
    function global:rustup { & $script:RustupExe @args }
}

Write-Host "* Using cargo: $script:CargoExe" -ForegroundColor Green
Write-Host "* Using rustc: $script:RustcExe" -ForegroundColor Green

# -- Rust version info -----------------------------------------------------
$rustVersionResult = $null
$rustVersion = ""

# PowerShell launched from WSL can fail to surface stdout/exit codes from
# direct `& exe args` invocations reliably. Use Start-Process for all external
# tools so the script behaves consistently in native Windows and WSL-driven
# sessions.
function Invoke-ProcessCapture {
    param(
        [string]$FilePath,
        [string[]]$ArgumentList = @()
    )

    $stdoutFile = [System.IO.Path]::GetTempFileName()
    $stderrFile = [System.IO.Path]::GetTempFileName()

    try {
        $process = Start-Process `
            -FilePath $FilePath `
            -ArgumentList $ArgumentList `
            -NoNewWindow `
            -Wait `
            -PassThru `
            -RedirectStandardOutput $stdoutFile `
            -RedirectStandardError $stderrFile

        $stdout = if (Test-Path $stdoutFile) {
            [System.IO.File]::ReadAllText($stdoutFile)
        } else {
            ""
        }
        $stderr = if (Test-Path $stderrFile) {
            [System.IO.File]::ReadAllText($stderrFile)
        } else {
            ""
        }

        return [pscustomobject]@{
            ExitCode = $process.ExitCode
            StdOut   = $stdout
            StdErr   = $stderr
        }
    } finally {
        Remove-Item $stdoutFile, $stderrFile -ErrorAction SilentlyContinue
    }
}

function Split-CommandTokens {
    param([string]$Command)

    $matches = [regex]::Matches($Command, '\"(?:\\.|[^\"])*\"|''(?:\\.|[^''])*''|\S+')
    $tokens = @()
    foreach ($match in $matches) {
        $token = $match.Value
        if (
            ($token.StartsWith('"') -and $token.EndsWith('"')) -or
            ($token.StartsWith("'") -and $token.EndsWith("'"))
        ) {
            $token = $token.Substring(1, $token.Length - 2)
        }
        $tokens += $token
    }
    return $tokens
}

function Resolve-ExternalCommand {
    param([string]$Command)

    $tokens = Split-CommandTokens $Command
    if ($tokens.Count -eq 0) {
        throw "Cannot execute an empty command."
    }

    $toolName = $tokens[0]
    $argumentList = if ($tokens.Count -gt 1) {
        @($tokens[1..($tokens.Count - 1)])
    } else {
        @()
    }

    switch ($toolName) {
        "cargo" {
            return [pscustomobject]@{
                FilePath     = $script:CargoExe
                ArgumentList = $argumentList
            }
        }
        "rustc" {
            return [pscustomobject]@{
                FilePath     = $script:RustcExe
                ArgumentList = $argumentList
            }
        }
        "rustup" {
            if (-not $script:RustupExe) {
                throw "rustup is not available."
            }
            return [pscustomobject]@{
                FilePath     = $script:RustupExe
                ArgumentList = $argumentList
            }
        }
        default {
            $commandInfo = Get-Command $toolName -ErrorAction Stop | Select-Object -First 1
            return [pscustomobject]@{
                FilePath     = $commandInfo.Source
                ArgumentList = $argumentList
            }
        }
    }
}

function Invoke-ResolvedCommand {
    param([string]$Command)

    $resolved = Resolve-ExternalCommand $Command
    return Invoke-ProcessCapture -FilePath $resolved.FilePath -ArgumentList $resolved.ArgumentList
}

$rustVersionResult = Invoke-ProcessCapture -FilePath $script:RustcExe -ArgumentList @("--version")
if ($rustVersionResult.ExitCode -eq 0) {
    $rustVersion = $rustVersionResult.StdOut.Trim()
}
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
        $result = Invoke-ResolvedCommand $Command
        $exitCode = $result.ExitCode
        $ErrorActionPreference = $oldErrorAction

        if ($exitCode -ne 0) {
            if ($result.StdOut) {
                $result.StdOut -split "(`r`n|`n|`r)" | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host $_ }
            }
            if ($result.StdErr) {
                $result.StdErr -split "(`r`n|`n|`r)" | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host $_ }
            }
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
        $result = Invoke-ResolvedCommand $Command
        $exitCode = $result.ExitCode
        $ErrorActionPreference = $oldErrorAction

        if ($exitCode -ne 0) {
            if ($result.StdOut) {
                $result.StdOut -split "(`r`n|`n|`r)" | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host $_ }
            }
            if ($result.StdErr) {
                $result.StdErr -split "(`r`n|`n|`r)" | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host $_ }
            }
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

# -- 3. Compile check (no default features) --------------------------------
# Catches missing #[cfg(feature)] gates. Full clippy is redundant here since
# lint issues would also appear in the all-features run above.
Run-Check "Compile check (no default features)" "cargo check --tests --no-default-features"

# -- 4. Tests (parallel: all features + no default features) ---------------
# Both test suites are independent — run them in parallel to halve wall time.
Write-Host "Running: Tests (all features + no default features) [parallel]" -ForegroundColor Blue

$repoRoot = (Get-Location).Path

$allFeaturesJob = Start-Job -ScriptBlock {
    param($CargoExe, $WorkDir)
    Set-Location $WorkDir
    $result = & $CargoExe test --all-features 2>&1
    [pscustomobject]@{
        Output   = ($result -join "`n")
        ExitCode = $LASTEXITCODE
    }
} -ArgumentList $script:CargoExe, $repoRoot

$noDefaultJob = Start-Job -ScriptBlock {
    param($CargoExe, $WorkDir, $TargetDir)
    Set-Location $WorkDir
    # Use a separate target dir to avoid lock contention with the parallel job.
    $env:CARGO_TARGET_DIR = $TargetDir
    $result = & $CargoExe test --no-default-features 2>&1
    [pscustomobject]@{
        Output   = ($result -join "`n")
        ExitCode = $LASTEXITCODE
    }
} -ArgumentList $script:CargoExe, $repoRoot, "target/no-default"

$startTime = Get-Date
$allFeaturesResult = Receive-Job -Job $allFeaturesJob -Wait
$noDefaultResult = Receive-Job -Job $noDefaultJob -Wait
Remove-Job $allFeaturesJob, $noDefaultJob
$duration = [math]::Round(((Get-Date) - $startTime).TotalSeconds)

$failed = $false
if ($allFeaturesResult.ExitCode -ne 0) {
    Write-Host "FAIL: Tests (all features)" -ForegroundColor Red
    Write-Host $allFeaturesResult.Output
    $failed = $true
} else {
    Write-Host "PASS: Tests (all features)" -ForegroundColor Green
}

if ($noDefaultResult.ExitCode -ne 0) {
    Write-Host "FAIL: Tests (no default features)" -ForegroundColor Red
    Write-Host $noDefaultResult.Output
    $failed = $true
} else {
    Write-Host "PASS: Tests (no default features)" -ForegroundColor Green
}

Write-Host "Tests completed (${duration}s)" -ForegroundColor $(if ($failed) { "Red" } else { "Green" })
Write-Host ""
if ($failed) {
    Write-Host "ERROR: Fix the test failures above before pushing." -ForegroundColor Red
    exit 1
}

# -- 6. Documentation ------------------------------------------------------
$env:RUSTDOCFLAGS = "-D warnings"
Run-Check "Documentation" "cargo doc --no-deps --document-private-items --all-features"

# -- 7. License check (cargo-deny) -----------------------------------------
Write-Host "Running license check..." -ForegroundColor Cyan
if ((Invoke-ResolvedCommand "cargo deny --version").ExitCode -eq 0) {
    Run-Check "License check (cargo deny)" "cargo deny check licenses"
} else {
    Write-Host "WARNING: cargo-deny not found. Installing..." -ForegroundColor Yellow
    try {
        $installResult = Invoke-ResolvedCommand "cargo install cargo-deny --locked"
        if ($installResult.StdOut) {
            $installResult.StdOut -split "(`r`n|`n|`r)" | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host $_ }
        }
        if ($installResult.StdErr) {
            $installResult.StdErr -split "(`r`n|`n|`r)" | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host $_ }
        }
        if ($installResult.ExitCode -eq 0) {
            Run-Check "License check (cargo deny)" "cargo deny check licenses"
        } else { throw "install failed" }
    } catch {
        Write-Host "WARNING: Could not install cargo-deny. Skipping license check." -ForegroundColor Yellow
        Write-Host "  Install manually: cargo install cargo-deny" -ForegroundColor Yellow
    }
}

# -- 8. Security audit (cargo-audit) ---------------------------------------
Write-Host "Running security audit..." -ForegroundColor Cyan
if ((Invoke-ResolvedCommand "cargo audit --version").ExitCode -eq 0) {
    Run-Check "Security audit" "cargo audit"
} else {
    Write-Host "WARNING: cargo-audit not found. Installing..." -ForegroundColor Yellow
    try {
        $installResult = Invoke-ResolvedCommand "cargo install cargo-audit --locked"
        if ($installResult.StdOut) {
            $installResult.StdOut -split "(`r`n|`n|`r)" | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host $_ }
        }
        if ($installResult.StdErr) {
            $installResult.StdErr -split "(`r`n|`n|`r)" | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host $_ }
        }
        if ($installResult.ExitCode -eq 0) {
            Run-Check "Security audit" "cargo audit"
        } else { throw "install failed" }
    } catch {
        Write-Host "WARNING: Could not install cargo-audit. Skipping security audit." -ForegroundColor Yellow
        Write-Host "  Install manually: cargo install cargo-audit" -ForegroundColor Yellow
    }
}

# -- 9. MSRV check (rust-version from Cargo.toml) --------------------------
$cargoToml = Get-Content "Cargo.toml" -Raw
$msrvMatch = [regex]::Match($cargoToml, '(?m)^\s*rust-version\s*=\s*"([^"]+)"')
if (-not $msrvMatch.Success) {
    Write-Host "WARNING: Could not determine rust-version from Cargo.toml. Skipping MSRV check." -ForegroundColor Yellow
    $msrv = $null
} else {
    $msrv = $msrvMatch.Groups[1].Value
}

if (-not $msrv) {
    Write-Host ""
    Write-Host "All CI checks passed!" -ForegroundColor Green
    Write-Host "Review any auto-fixes, then push." -ForegroundColor Blue
    exit 0
}

Write-Host "Checking MSRV ($msrv)..." -ForegroundColor Cyan
if ($script:RustupExe) {
    $toolchainResult = Invoke-ProcessCapture -FilePath $script:RustupExe -ArgumentList @("toolchain", "list")
    $toolchains = $toolchainResult.StdOut
    $hasMsrv = $toolchains -match [regex]::Escape($msrv)

    if (-not $hasMsrv) {
        Write-Host "Installing Rust $msrv toolchain..." -ForegroundColor Yellow
        $installResult = Invoke-ProcessCapture -FilePath $script:RustupExe -ArgumentList @("toolchain", "install", $msrv, "--profile", "minimal")
        if ($installResult.StdOut) {
            $installResult.StdOut -split "(`r`n|`n|`r)" | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host $_ }
        }
        if ($installResult.StdErr) {
            $installResult.StdErr -split "(`r`n|`n|`r)" | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host $_ }
        }
        if ($installResult.ExitCode -ne 0) {
            Write-Host "WARNING: Could not install Rust $msrv. Skipping MSRV check." -ForegroundColor Yellow
            $hasMsrv = $false
        } else {
            $hasMsrv = $true
        }
    }

    if ($hasMsrv) {
        # Ensure clippy is available for MSRV
        $componentResult = Invoke-ProcessCapture -FilePath $script:RustupExe -ArgumentList @("component", "list", "--toolchain", $msrv)
        $clippy = $componentResult.StdOut | Select-String "clippy.*(installed)"
        if (-not $clippy) {
            $componentInstallResult = Invoke-ProcessCapture -FilePath $script:RustupExe -ArgumentList @("component", "add", "clippy", "--toolchain", $msrv)
            if ($componentInstallResult.StdOut) {
                $componentInstallResult.StdOut -split "(`r`n|`n|`r)" | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host $_ }
            }
            if ($componentInstallResult.StdErr) {
                $componentInstallResult.StdErr -split "(`r`n|`n|`r)" | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host $_ }
            }
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
