# Requires -RunAsAdministrator

Write-Host "=== Configuring Secure ACL Permissions for Diviora Directories ===" -ForegroundColor Cyan

$configDir = "C:\ProgramData\Diviora\config"
New-Item -Path $configDir -ItemType Directory -Force | Out-Null

$acl = Get-Acl $configDir
$rule = New-Object System.Security.AccessControl.FileSystemAccessRule(
    "BUILTIN\Users",
    "Read, Write",
    "ContainerInherit, ObjectInherit",
    "None",
    "Allow"
)
$acl.SetAccessRule($rule)
Set-Acl -Path $configDir -AclObject $acl

Write-Host "Configured secure Read/Write permissions for non-elevated user runtime at $configDir" -ForegroundColor Green