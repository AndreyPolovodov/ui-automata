# Install automata-agent from the latest GitHub release.
#
# Usage:
#   PowerShell -Command "iwr https://raw.githubusercontent.com/visioncortex/ui-automata/main/install/install-windows.ps1 | iex"
#
# Optional env overrides:
#   $env:AUTOMATA_DIR  — installation directory (default: $HOME\.ui-automata)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Dest  = if ($env:AUTOMATA_DIR) { $env:AUTOMATA_DIR } else { "$env:USERPROFILE\.ui-automata" }
$Repo  = "visioncortex/ui-automata"
$Api   = "https://api.github.com/repos/$Repo/releases/latest"

Write-Host "Fetching latest release from $Repo ..."
$release = Invoke-RestMethod -Uri $Api -Headers @{ "User-Agent" = "automata-installer" }
$tag     = $release.tag_name
$asset   = $release.assets | Where-Object { $_.name -like "automata-windows-*.zip" } | Select-Object -First 1

if (-not $asset) {
    Write-Error "No automata-windows-*.zip asset found in release $tag"
    exit 1
}

$zip = "$env:TEMP\automata-$tag.zip"
Write-Host "Downloading $($asset.name) ($tag) ..."
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip

Write-Host "Extracting to $Dest ..."
if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest }
Add-Type -Assembly System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::ExtractToDirectory($zip, $Dest)
Remove-Item $zip

Write-Host "Running self-test ..."
$agent = Join-Path $Dest "automata-agent.exe"
if (-not (Test-Path $agent)) {
    Write-Error "automata-agent.exe not found in $Dest"
    exit 1
}
& $agent --self-test
if ($LASTEXITCODE -ne 0) {
    Write-Error "Self-test failed (exit $LASTEXITCODE)"
    exit $LASTEXITCODE
}

Write-Host ""
Write-Host "OK — automata-agent $tag installed to $Dest"
