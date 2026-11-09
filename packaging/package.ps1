# Builds the release assets into .\dist:
#   Sweepr-v<ver>-x64-Setup.exe     (Inno Setup; skipped with a warning when ISCC is absent)
#   Sweepr-v<ver>-x64-Portable.zip  (exe + docs + portable.flag)
#   SHA256SUMS.txt                  (covers every asset produced)
# Run after: cargo build --release --locked
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\release\sweepr.exe'
if (-not (Test-Path $exe)) { throw "Missing $exe. Build first." }
$ver = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$dist = Join-Path $root 'dist'
Remove-Item $dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $dist | Out-Null

# Portable zip
$stage = Join-Path $dist "Sweepr-v$ver-x64-Portable"
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item $exe $stage
Copy-Item (Join-Path $root 'README.md'), (Join-Path $root 'LICENSE'), (Join-Path $root 'CHANGELOG.md'), (Join-Path $root 'CREDITS.md') $stage
New-Item -ItemType File -Force -Path (Join-Path $stage 'portable.flag') | Out-Null
Compress-Archive -Path "$stage\*" -DestinationPath (Join-Path $dist "Sweepr-v$ver-x64-Portable.zip")
Remove-Item $stage -Recurse -Force

# Setup exe (Inno Setup 6)
$iscc = Get-Command ISCC.exe -ErrorAction SilentlyContinue
if (-not $iscc) {
    $candidates = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    )
    $found = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
    if ($found) { $iscc = Get-Item $found }
}
if ($iscc) {
    $isccPath = if ($iscc.Source) { $iscc.Source } else { $iscc.FullName }
    & $isccPath "/DVersion=$ver" "/DExe=$exe" (Join-Path $PSScriptRoot 'Sweepr.iss')
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }
} else {
    Write-Warning 'ISCC.exe not found: Setup.exe skipped. Install Inno Setup 6 to build it.'
}

Get-ChildItem $dist -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | Sort-Object Name | ForEach-Object {
    $h = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
    "$h  $($_.Name)"
} | Set-Content (Join-Path $dist 'SHA256SUMS.txt') -Encoding ASCII
