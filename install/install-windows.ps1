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

# Add $Dest to the user PATH if not already present.
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -notlike "*$Dest*") {
    [Environment]::SetEnvironmentVariable("PATH", "$userPath;$Dest", "User")
    Write-Host "Added $Dest to user PATH"
} else {
    Write-Host "$Dest already in user PATH"
}

# Download and extract the workflow library.
$wfAsset = $release.assets | Where-Object { $_.name -eq "workflow-library.zip" } | Select-Object -First 1
if ($wfAsset) {
    $wfZip = "$env:TEMP\workflow-library.zip"
    $wfDest = Join-Path $Dest "workflows"
    Write-Host "Downloading workflow library ..."
    Invoke-WebRequest -Uri $wfAsset.browser_download_url -OutFile $wfZip
    if (Test-Path $wfDest) { Remove-Item -Recurse -Force $wfDest }
    [System.IO.Compression.ZipFile]::ExtractToDirectory($wfZip, $wfDest)
    Remove-Item $wfZip
    Write-Host "Workflows extracted to $wfDest"
} else {
    Write-Host "No workflow-library.zip found in release $tag, skipping"
}

Write-Host ""
Write-Host "automata-agent $tag installed to $Dest"
