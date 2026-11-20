# Builds the OFP Studio installer for Windows (NSIS, x64), signed with Azure Trusted Signing when the
# signing account is given. Run on Windows from the repository:
#   powershell -ExecutionPolicy Bypass -File scripts\app-build-windows.ps1 -Endpoint <url> -Account <name> -Profile <name>
# Without them the installer is built unsigned. Output: target\release\bundle\nsis\*.exe
param([string]$Endpoint, [string]$Account, [string]$Profile)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
npm ci --no-audit --no-fund
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Remove-Item -Recurse -Force target\release\bundle\nsis -ErrorAction SilentlyContinue
$extra = @()
if ($Endpoint -and $Account -and $Profile) {
  # Tauri splits the command on spaces and passes quotes through: the path must have no spaces
  $sign = (Resolve-Path scripts\sign-windows.ps1).Path
  if ($sign -match ' ') { throw "The repository path has a space ($sign): signing cannot be called from it" }
  $config = @{ bundle = @{ windows = @{ signCommand = "powershell -NoProfile -ExecutionPolicy Bypass -File $sign -Endpoint $Endpoint -Account $Account -Profile $Profile %1" } } } | ConvertTo-Json -Depth 5 -Compress
  $file = Join-Path $env:TEMP 'ofp-windows-sign.json'
  Set-Content -Path $file -Value $config -Encoding ascii
  $extra = @('--config', $file)
  Write-Host "Signing with $Account / $Profile"
} else {
  Write-Host 'No signing account given: building unsigned.'
}
Push-Location crates\ofp-studio-app
npx tauri build --bundles nsis @extra
$code = $LASTEXITCODE
Pop-Location
exit $code
