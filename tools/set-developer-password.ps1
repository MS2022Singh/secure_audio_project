<#
.SYNOPSIS
    Choose a new Diviora developer password (developer only).

.DESCRIPTION
    Asks for the new password twice and creates developer_access.json on your Desktop.
    The file holds only salted PBKDF2-SHA256 hashes (600 000 rounds) of the password and of a
    new recovery key, never the password itself, so it is safe to put in the repository.

    Then replace src-tauri\developer_access.json with it (or send it to your build helper),
    and publish an update: from that version on, only the new password opens the developer
    panel on every device. The recovery key printed below also unlocks it; keep it offline.

    Run (normal PowerShell, no admin needed):
        powershell -ExecutionPolicy Bypass -File tools\set-developer-password.ps1
#>
param([string]$OutFile = (Join-Path ([Environment]::GetFolderPath('Desktop')) 'developer_access.json'), [switch]$SelfTest)
$ErrorActionPreference = 'Stop'
$Rounds = 600000

function Plain([Security.SecureString]$s) {
    $b = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($s)
    try { [Runtime.InteropServices.Marshal]::PtrToStringBSTR($b) } finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($b) }
}
function Hex([byte[]]$b) { ($b | ForEach-Object { $_.ToString('x2') }) -join '' }
function RandomBytes([int]$n) { $b = New-Object byte[] $n; [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($b); $b }
function Pbkdf2([string]$secret, [byte[]]$salt) {
    $k = New-Object Security.Cryptography.Rfc2898DeriveBytes(
        [Text.Encoding]::UTF8.GetBytes($secret), $salt, $Rounds, [Security.Cryptography.HashAlgorithmName]::SHA256)
    try { $k.GetBytes(32) } finally { $k.Dispose() }
}

if ($SelfTest) {
    # CI check: must match the app's PBKDF2-HMAC-SHA256 (same vector checked in Python).
    $got = Hex (Pbkdf2 'Test-Passw0rd!' ([byte[]](0..15)))
    if ($got -ne '62110006f4d31f35e981da436a470924276fb721f26408baf104fea8e707b05f') { Write-Host "SELF-TEST FAILED: $got"; exit 1 }
    Write-Host 'SELF-TEST OK'; exit 0
}

$p1 = Plain (Read-Host 'New developer password' -AsSecureString)
$p2 = Plain (Read-Host 'Repeat it' -AsSecureString)
if ($p1 -ne $p2) { throw 'The two entries are different. Nothing was created.' }
$classes = @(($p1 -cmatch '[a-z]'), ($p1 -cmatch '[A-Z]'), ($p1 -match '[0-9]'), ($p1 -match '[^a-zA-Z0-9]')) | Where-Object { $_ }
if ($p1.Length -lt 12 -or $classes.Count -lt 3) {
    throw 'Use at least 12 characters and three of: lowercase, uppercase, digits, symbols.'
}

$alphabet = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789'
$r = -join ((RandomBytes 20) | ForEach-Object { $alphabet[$_ % 32] })
$recovery = 'DVR-{0}-{1}-{2}-{3}-{4}' -f $r.Substring(0,4), $r.Substring(4,4), $r.Substring(8,4), $r.Substring(12,4), $r.Substring(16,4)

Write-Host 'Hashing (takes a few seconds)...'
$salt = RandomBytes 16
$rsalt = RandomBytes 16
$record = [ordered]@{
    version       = 2
    rounds        = $Rounds
    salt          = Hex $salt
    password_hash = Hex (Pbkdf2 $p1 $salt)
    recovery_salt = Hex $rsalt
    recovery_hash = Hex (Pbkdf2 $recovery $rsalt)
}
[IO.File]::WriteAllText($OutFile, ($record | ConvertTo-Json) + "`n", (New-Object Text.UTF8Encoding $false))

Write-Host ''
Write-Host "Created: $OutFile" -ForegroundColor Green
Write-Host "RECOVERY KEY (write it down, keep it offline): $recovery" -ForegroundColor Yellow
Write-Host 'Next: replace src-tauri\developer_access.json with this file and publish an update.'
