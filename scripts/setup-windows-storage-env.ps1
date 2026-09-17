<#
.SYNOPSIS
    One-time Windows dev-machine setup for LifeOS's encrypted storage
    build dependency (SQLCipher via rusqlite's `bundled-sqlcipher`
    feature, which links against an external OpenSSL).

.DESCRIPTION
    `bundled-sqlcipher` needs OpenSSL headers/import libraries to build
    (build-time only - the resulting lifeos.exe statically links OpenSSL
    and has NO runtime DLL dependency on it, verified via
    `dumpbin /dependents` - see
    docs/reports/REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md).

    This script does NOT hardcode a path into the repository. It checks
    the standard install location for a prebuilt Windows OpenSSL dev
    package, fails clearly if missing, and - if found - sets
    OPENSSL_DIR / OPENSSL_LIB_DIR / OPENSSL_INCLUDE_DIR as PERSISTENT
    USER environment variables (no admin rights required; these survive
    across terminal sessions and reboots, and any later `cargo build`
    picks them up automatically without a committed config file).

    Run once per machine, before building src-tauri:
        pwsh -File scripts/setup-windows-storage-env.ps1

    Then open a NEW terminal (environment variables set this way are
    not visible in the terminal that ran the script itself) and run
    `cargo build` / `npm run tauri build` as usual.

.NOTES
    Never contains secrets. Only reads a public install path and writes
    three build-tooling path variables.
#>

$ErrorActionPreference = "Stop"

$OpenSslRoot = "C:\Program Files\OpenSSL-Win64"
$LibDir = Join-Path $OpenSslRoot "lib\VC\x64\MD"
$IncludeDir = Join-Path $OpenSslRoot "include"

if (-not (Test-Path $OpenSslRoot)) {
    Write-Error @"
OpenSSL dev package not found at: $OpenSslRoot

LifeOS's encrypted storage (SQLCipher via rusqlite's bundled-sqlcipher
feature) needs a Windows OpenSSL development package to BUILD (not to
run - the shipped app has no runtime dependency on it).

Install it with:
    winget install --id ShiningLight.OpenSSL.Dev --accept-package-agreements --accept-source-agreements

Then re-run this script.
"@
    exit 1
}

if (-not (Test-Path $LibDir)) {
    Write-Error "Expected library directory not found: $LibDir`nThe OpenSSL install at $OpenSslRoot does not match the expected layout (lib\VC\x64\MD). Was a different package/version installed?"
    exit 1
}

if (-not (Test-Path $IncludeDir)) {
    Write-Error "Expected include directory not found: $IncludeDir"
    exit 1
}

[Environment]::SetEnvironmentVariable("OPENSSL_DIR", $OpenSslRoot, "User")
[Environment]::SetEnvironmentVariable("OPENSSL_LIB_DIR", $LibDir, "User")
[Environment]::SetEnvironmentVariable("OPENSSL_INCLUDE_DIR", $IncludeDir, "User")

Write-Output "OpenSSL found at $OpenSslRoot - set as persistent user environment variables:"
Write-Output "  OPENSSL_DIR = $OpenSslRoot"
Write-Output "  OPENSSL_LIB_DIR = $LibDir"
Write-Output "  OPENSSL_INCLUDE_DIR = $IncludeDir"
Write-Output ""
Write-Output "Open a NEW terminal window for these to take effect, then build normally:"
Write-Output "  cd src-tauri; cargo build --release"
