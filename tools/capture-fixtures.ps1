# Sweepr fixture capture. Run in a normal (non-admin) PowerShell. Read-only: it only lists outdated packages.
# Output goes to .\captured\ ; attach those files so the synthetic fixtures can be replaced with real ones.
$ErrorActionPreference = 'Continue'
$out = Join-Path (Get-Location) 'captured'
New-Item -ItemType Directory -Force -Path $out | Out-Null
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

"Windows: " + [System.Environment]::OSVersion.VersionString + " | UI culture: " + (Get-UICulture).Name |
    Set-Content (Join-Path $out 'env.txt') -Encoding UTF8

if (Get-Command winget -ErrorAction SilentlyContinue) {
    winget --version | Set-Content (Join-Path $out 'winget_version.txt') -Encoding UTF8
    winget upgrade --accept-source-agreements --disable-interactivity 2>&1 |
        Out-String | Set-Content (Join-Path $out 'winget_upgrade.real.txt') -Encoding UTF8
} else { 'winget not found' | Set-Content (Join-Path $out 'winget_upgrade.real.txt') }

if (Get-Command choco -ErrorAction SilentlyContinue) {
    choco --version | Set-Content (Join-Path $out 'choco_version.txt') -Encoding UTF8
    choco outdated --limit-output 2>&1 |
        Out-String | Set-Content (Join-Path $out 'choco_outdated.real.txt') -Encoding UTF8
} else { 'choco not found' | Set-Content (Join-Path $out 'choco_outdated.real.txt') }

if (Get-Command scoop -ErrorAction SilentlyContinue) {
    scoop status 2>&1 |
        Out-String | Set-Content (Join-Path $out 'scoop_status.real.txt') -Encoding UTF8
} else { 'scoop not found' | Set-Content (Join-Path $out 'scoop_status.real.txt') }

Write-Host "Done. Files are in $out"
