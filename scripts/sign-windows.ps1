# Signs one Windows file with Azure Trusted Signing (Tauri's bundle.windows.signCommand calls this for every
# file it signs). Needs the TrustedSigning PowerShell module, .NET 8 and the service principal in
# AZURE_TENANT_ID / AZURE_CLIENT_ID / AZURE_CLIENT_SECRET. Signing fails with "Access is denied" when nobody
# is logged in to the machine.
param(
  [Parameter(Mandatory = $true)][string]$Endpoint,
  [Parameter(Mandatory = $true)][string]$Account,
  [Parameter(Mandatory = $true)][string]$Profile,
  [Parameter(Mandatory = $true, Position = 0)][string]$File
)
$ErrorActionPreference = 'Stop'
Invoke-TrustedSigning -Endpoint $Endpoint -CodeSigningAccountName $Account -CertificateProfileName $Profile `
  -Files $File -FileDigest SHA256 -TimestampRfc3161 'http://timestamp.acs.microsoft.com' -TimestampDigest SHA256
if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
