#!/bin/bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2025-present Iron Curtain contributors

# ci-local.sh - Local CI validation for cnc-formats
# Run all CI checks locally before pushing.
# Mirrors the GitHub Actions workflow in .github/workflows/ci.yml.

set -e

echo "=== cnc-formats - Local CI ==="

# -- Locate cargo ----------------------------------------------------------
if ! command -v cargo &> /dev/null; then
    CARGO_PATHS=(
        "$HOME/.cargo/bin/cargo"
        "$HOME/.cargo/bin/cargo.exe"
        "/c/Users/$(whoami)/.cargo/bin/cargo.exe"
    )

    for cargo_path in "${CARGO_PATHS[@]}"; do
        if [[ -x "$cargo_path" ]]; then
            export PATH="$(dirname "$cargo_path"):$PATH"
            echo "* Found cargo at: $cargo_path"
            break
        fi
    done

    if ! command -v cargo &> /dev/null; then
        echo "ERROR: cargo not found. Install Rust from https://rustup.rs/"
        exit 1
    fi
fi

echo "* Using cargo: $(command -v cargo)"

# -- Rust version info -----------------------------------------------------
RUST_VERSION=$(rustc --version)
echo "Rust version: $RUST_VERSION"

if echo "$RUST_VERSION" | grep -q "nightly"; then
    echo "WARNING: You are using nightly Rust, but GitHub Actions uses stable!"
    echo "   Consider testing with: rustup default stable"
fi
echo

# -- Helpers ---------------------------------------------------------------

run_check() {
    local name="$1"
    local command="$2"

    echo "Running: $name"
    echo "Command: $command"

    local start_time
    start_time=$(date +%s)

    if eval "$command"; then
        local end_time
        end_time=$(date +%s)
        local duration=$((end_time - start_time))
        echo "PASS: $name (${duration}s)"
        echo
    else
        local end_time
        end_time=$(date +%s)
        local duration=$((end_time - start_time))
        echo "FAIL: $name (${duration}s)"
        echo "ERROR: Fix the issue above before pushing."
        exit 1
    fi
}

run_fix() {
    local name="$1"
    local command="$2"

    echo "Auto-fixing: $name"
    echo "Command: $command"

    local start_time
    start_time=$(date +%s)

    if eval "$command"; then
        local end_time
        end_time=$(date +%s)
        local duration=$((end_time - start_time))
        echo "DONE: $name (${duration}s)"
        echo
    else
        local end_time
        end_time=$(date +%s)
        local duration=$((end_time - start_time))
        echo "WARNING: $name failed (${duration}s) -- continuing"
        echo
    fi
}

# -- Pre-flight: project root ----------------------------------------------
if [[ ! -f "Cargo.toml" ]]; then
    echo "ERROR: Cargo.toml not found. Run this from the project root."
    exit 1
fi

# -- UTF-8 encoding validation ---------------------------------------------
echo "Validating UTF-8 encoding..."

check_utf8() {
    local file="$1"

    if [[ ! -f "$file" ]]; then
        echo "ERROR: File not found: $file"
        return 1
    fi

    if command -v file >/dev/null 2>&1; then
        local file_output
        file_output=$(file "$file")
        if echo "$file_output" | grep -q "UTF-8\|ASCII\|text\|[Ss]ource"; then
            echo "  OK: $file"
            return 0
        else
            echo "ERROR: $file is not valid UTF-8"
            return 1
        fi
    fi

    # Fallback: assume OK if 'file' command unavailable
    echo "  OK: $file (assumed)"
    return 0
}

check_no_bom() {
    local file="$1"
    if command -v xxd >/dev/null 2>&1; then
        if head -c 3 "$file" | xxd | grep -qE "ef[ ]?bb[ ]?bf"; then
            echo "ERROR: $file has UTF-8 BOM (remove it)"
            return 1
        fi
    elif command -v od >/dev/null 2>&1; then
        if head -c 3 "$file" | od -t x1 | grep -qE "ef[ ]?bb[ ]?bf"; then
            echo "ERROR: $file has UTF-8 BOM (remove it)"
            return 1
        fi
    fi
    return 0
}

check_utf8 "README.md" || exit 1
check_no_bom "README.md" || exit 1
check_utf8 "Cargo.toml" || exit 1

if [[ -d "src" ]]; then
    find src -name "*.rs" -type f | while read -r file; do
        check_utf8 "$file" || exit 1
    done
    echo "  All Rust source files: OK"
fi
echo

# -- Auto-fix --------------------------------------------------------------
echo "Auto-fixing formatting and lint..."
run_fix "Format" "cargo fmt"
run_fix "Clippy auto-fix" "cargo clippy --fix --allow-dirty --allow-staged --all-targets --all-features"
run_fix "Format (post-clippy)" "cargo fmt"

echo "Running CI checks..."
echo

# -- 1. Format check -------------------------------------------------------
run_check "Format check" "cargo fmt --check"

# -- 2. Clippy (all features) ----------------------------------------------
run_check "Clippy (all features)" "cargo clippy --all-targets --all-features -- -D warnings"

# -- 3. Compile check (no default features) --------------------------------
# Catches missing #[cfg(feature)] gates. Full clippy is redundant here since
# lint issues would also appear in the all-features run above.
run_check "Compile check (no default features)" "cargo check --tests --no-default-features"

# -- 4. Tests (parallel: all features + no default features) ---------------
# Both test suites are independent — run them in parallel to halve wall time.
echo "Running: Tests (all features + no default features) [parallel]"

start_time=$(date +%s)
cargo test --all-features &
pid_all=$!

CARGO_TARGET_DIR=target/no-default cargo test --no-default-features &
pid_nodef=$!

test_failed=0
if ! wait $pid_all; then
    echo "FAIL: Tests (all features)"
    test_failed=1
else
    echo "PASS: Tests (all features)"
fi

if ! wait $pid_nodef; then
    echo "FAIL: Tests (no default features)"
    test_failed=1
else
    echo "PASS: Tests (no default features)"
fi

end_time=$(date +%s)
duration=$((end_time - start_time))
echo "Tests completed (${duration}s)"
echo

if [[ $test_failed -ne 0 ]]; then
    echo "ERROR: Fix the test failures above before pushing."
    exit 1
fi

# -- 6. Documentation ------------------------------------------------------
run_check "Documentation" "RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --document-private-items --all-features"

# -- 7. License check (cargo-deny) -----------------------------------------
echo "Running license check..."
if command -v cargo-deny &> /dev/null; then
    run_check "License check (cargo deny)" "cargo deny check licenses"
else
    echo "WARNING: cargo-deny not found. Installing..."
    if cargo install cargo-deny --locked; then
        run_check "License check (cargo deny)" "cargo deny check licenses"
    else
        echo "WARNING: Could not install cargo-deny. Skipping license check."
        echo "  Install manually: cargo install cargo-deny"
    fi
fi

# -- 8. Security audit (cargo-audit) ---------------------------------------
echo "Running security audit..."
if command -v cargo-audit &> /dev/null; then
    run_check "Security audit" "cargo audit"
else
    echo "WARNING: cargo-audit not found. Installing..."
    if cargo install cargo-audit --locked; then
        run_check "Security audit" "cargo audit"
    else
        echo "WARNING: Could not install cargo-audit. Skipping security audit."
        echo "  Install manually: cargo install cargo-audit"
    fi
fi

# -- 9. MSRV check (rust-version from Cargo.toml) --------------------------
MSRV="1.86"
echo "Checking MSRV ($MSRV)..."
if command -v rustup &> /dev/null; then
    HAS_MSRV=false
    if rustup toolchain list | grep -q "$MSRV"; then
        HAS_MSRV=true
    else
        echo "Installing Rust $MSRV toolchain..."
        if rustup toolchain install "$MSRV" --profile minimal; then
            HAS_MSRV=true
        else
            echo "WARNING: Could not install Rust $MSRV. Skipping MSRV check."
        fi
    fi

    if $HAS_MSRV; then
        # Ensure clippy is available for MSRV
        if ! rustup component list --toolchain "$MSRV" | grep -q "clippy.*(installed)"; then
            rustup component add clippy --toolchain "$MSRV"
        fi

        export CARGO_TARGET_DIR=target/msrv
        run_check "MSRV compile (Rust $MSRV)" "rustup run $MSRV cargo check --all-targets --all-features"
        run_check "MSRV clippy (Rust $MSRV)" "rustup run $MSRV cargo clippy --all-targets --all-features -- -D warnings"
        run_check "MSRV test (Rust $MSRV)" "rustup run $MSRV cargo test --all-features"
        unset CARGO_TARGET_DIR
    fi
else
    echo "WARNING: rustup not found. Skipping MSRV check."
fi

# -- Done ------------------------------------------------------------------
echo
echo "All CI checks passed!"
echo "Review any auto-fixes, then push."
