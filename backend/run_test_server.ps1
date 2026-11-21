<#
.SYNOPSIS
    Free local licence server for testing (no hosting needed).

.DESCRIPTION
    - Creates test secrets once (backend\test-secrets.json, kept out of git).
    - Points this PC's Diviora app and sync service at http://127.0.0.1:9000 and writes the
      developer credentials, so the developer panel can create invites and publish configs.
    - Starts the server in this window (Ctrl+C to stop).

    Run in an Administrator PowerShell from the project folder:
        powershell -ExecutionPolicy Bypass -File backend\run_test_server.ps1
    Requires Python 3.10+ (python.org).

    Only for testing on one PC: phones and other PCs can't reach 127.0.0.1. For real users,
    deploy the server behind HTTPS (backend/README.md).
#>
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

python -m pip install -q -r requirements.txt

$secretsFile = Join-Path $PSScriptRoot 'test-secrets.json'
if (-not (Test-Path $secretsFile)) {
    $rand = { param($n) $b = New-Object byte[] $n; [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($b); $b }
    @{
        DIVIORA_ADMIN_TOKEN = [Convert]::ToBase64String((& $rand 32)).TrimEnd('=').Replace('+','-').Replace('/','_')
        DIVIORA_SECRET      = [Convert]::ToBase64String((& $rand 32))
        DIVIORA_CONFIG_KEY  = [Convert]::ToBase64String((& $rand 32))
    } | ConvertTo-Json | Set-Content $secretsFile
    Write-Host "Test secrets created: $secretsFile" -ForegroundColor Green
}
$s = Get-Content $secretsFile -Raw | ConvertFrom-Json
$env:DIVIORA_ADMIN_TOKEN = $s.DIVIORA_ADMIN_TOKEN
$env:DIVIORA_SECRET      = $s.DIVIORA_SECRET
$env:DIVIORA_CONFIG_KEY  = $s.DIVIORA_CONFIG_KEY
$env:DIVIORA_DB          = Join-Path $PSScriptRoot 'test-licences.db'
$url = 'http://127.0.0.1:9000'

# Developer credentials (this PC only): invites + publishing from the developer panel.
$devDir = Join-Path $env:LOCALAPPDATA 'Diviora'
New-Item -ItemType Directory -Force -Path $devDir | Out-Null
@{ server_url = $url; admin_token = $s.DIVIORA_ADMIN_TOKEN } | ConvertTo-Json | Set-Content (Join-Path $devDir 'developer.json')

# Client: activation and the sync service use the local server.
$clientDir = Join-Path $env:ProgramData 'Diviora\client'
New-Item -ItemType Directory -Force -Path $clientDir | Out-Null
$clientFile = Join-Path $clientDir 'client.json'
$client = if (Test-Path $clientFile) { Get-Content $clientFile -Raw | ConvertFrom-Json } else { [pscustomobject]@{} }
$client | Add-Member -NotePropertyName server_url -NotePropertyValue $url -Force
$client | ConvertTo-Json | Set-Content $clientFile

Write-Host ""
Write-Host "Local licence server: $url" -ForegroundColor Cyan
Write-Host "Create a licence key for testing (in another PowerShell window):"
Write-Host "  Invoke-RestMethod -Method Post -Uri $url/api/v3/admin/licenses -Headers @{Authorization='Bearer $($s.DIVIORA_ADMIN_TOKEN)'} -ContentType 'application/json' -Body '{""days"":30}'"
Write-Host "Or use 'Generate Free Key Token' in the developer panel, and activate with it on the Consumer Deck."
Write-Host "Stop the server with Ctrl+C."
Write-Host ""
python server.py
