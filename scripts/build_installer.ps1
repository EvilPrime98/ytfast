<#
Build the Windows installer locally (mirrors the release workflow).

  scripts\build_installer.ps1 [-Version 0.1.0] [-SkipStage]

Output: dist\YTFast-Setup-<version>.exe
Requires Inno Setup 6 (winget install JRSoftware.InnoSetup) and Git Bash.
#>
param(
    [string]$Version,
    [switch]$SkipStage
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

if (-not $Version) {
    $Version = (Select-String -Path "$root\Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' |
        Select-Object -First 1).Matches[0].Groups[1].Value
}

$iscc = @(
    (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source,
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) {
    throw 'ISCC.exe not found. Install it with: winget install JRSoftware.InnoSetup'
}

if (-not $SkipStage) {
    Write-Host '==> staging app folder'
    # Relative path from the repo root: bash would eat the backslashes of an absolute Windows path.
    # Git Bash started from here may not have cargo on PATH (and `bash` may resolve to WSL), so be explicit.
    $cargo = (Get-Command cargo.exe -ErrorAction SilentlyContinue).Source
    if (-not $cargo) { $cargo = "$env:USERPROFILE\.cargo\bin\cargo.exe" }
    if (-not (Test-Path $cargo)) { throw 'cargo.exe not found. Install Rust from https://rustup.rs' }
    $env:PATH = "$(Split-Path -Parent $cargo);$env:PATH"
    $bash = @(
        "$env:ProgramFiles\Git\bin\bash.exe",
        "${env:ProgramFiles(x86)}\Git\bin\bash.exe",
        "$env:LOCALAPPDATA\Programs\Git\bin\bash.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $bash) { $bash = 'bash' }
    Push-Location $root
    try { & $bash packaging/stage.sh } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw 'packaging/stage.sh failed' }
} elseif (-not (Test-Path "$root\dist\YTFast\ytfast.exe")) {
    throw 'dist\YTFast\ytfast.exe missing; run without -SkipStage'
}

Write-Host "==> ISCC $Version"
& $iscc "/DAppVersion=$Version" "$root\packaging\windows\ytfast.iss"
if ($LASTEXITCODE -ne 0) { throw 'ISCC failed' }
Write-Host "==> $root\dist\YTFast-Setup-$Version.exe"
