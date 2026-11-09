# Builds the portable zip and SHA256SUMS into .\dist. Run after: cargo build --release --locked
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\release\sweepr.exe'
if (-not (Test-Path $exe)) { throw "Missing $exe. Build first." }
$ver = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$dist = Join-Path $root 'dist'
$stage = Join-Path $dist "Sweepr-$ver"
Remove-Item $dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item $exe $stage
Copy-Item (Join-Path $root 'README.md'), (Join-Path $root 'LICENSE'), (Join-Path $root 'CHANGELOG.md'), (Join-Path $root 'CREDITS.md') $stage
Compress-Archive -Path $stage -DestinationPath (Join-Path $dist "Sweepr-$ver-portable.zip")
Remove-Item $stage -Recurse -Force
# MSI: needs WiX v4 (dotnet tool 'wix'). Skipped when absent.
if (Get-Command wix -ErrorAction SilentlyContinue) {
    wix build (Join-Path $PSScriptRoot 'Sweepr.wxs') -d "Version=$ver" -d "Exe=$exe" -o (Join-Path $dist "Sweepr-$ver-x64.msi")
} else { Write-Host 'wix not found: MSI skipped' }
Get-ChildItem $dist -File | ForEach-Object {
    $h = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
    "$h  $($_.Name)"
} | Set-Content (Join-Path $dist 'SHA256SUMS') -Encoding ASCII
