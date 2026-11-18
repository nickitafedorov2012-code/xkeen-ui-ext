param(
    [switch]$Restore
)

$ErrorActionPreference = 'Stop'
$taskName = 'XKeen_Restore_Default_Interface_Metrics'
$wcmPolicyPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WcmSvc\GroupPolicy'

if ($Restore) {
    Set-NetIPInterface -InterfaceAlias 'Waffle' -AddressFamily IPv4 -AutomaticMetric Enabled
    Set-NetIPInterface -InterfaceAlias 'Ethernet' -AddressFamily IPv4 -AutomaticMetric Enabled
    Enable-NetAdapterBinding -Name 'Ethernet' -ComponentID 'ms_tcpip6' -ErrorAction SilentlyContinue
    Set-NetIPInterface -InterfaceAlias 'Ethernet' -AddressFamily IPv6 -AutomaticMetric Enabled -ErrorAction SilentlyContinue
    Remove-ItemProperty -Path $wcmPolicyPath -Name 'fMinimizeConnections' -ErrorAction SilentlyContinue
    Clear-DnsClientCache -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
    Write-Host '[OK] Default interface metrics and IPv6/WcmSvc settings restored.' -ForegroundColor Green
    exit 0
}

Set-NetIPInterface -InterfaceAlias 'Waffle' -AddressFamily IPv4 -AutomaticMetric Disabled -InterfaceMetric 5
Set-NetIPInterface -InterfaceAlias 'Ethernet' -AddressFamily IPv4 -AutomaticMetric Disabled -InterfaceMetric 500
Set-NetIPInterface -InterfaceAlias 'Ethernet' -AddressFamily IPv6 -AutomaticMetric Disabled -InterfaceMetric 500 -ErrorAction SilentlyContinue
Disable-NetAdapterBinding -Name 'Ethernet' -ComponentID 'ms_tcpip6' -ErrorAction SilentlyContinue

if (-not (Test-Path $wcmPolicyPath)) {
    New-Item -Path $wcmPolicyPath -Force | Out-Null
}
Set-ItemProperty -Path $wcmPolicyPath -Name 'fMinimizeConnections' -Type DWord -Value 0
Clear-DnsClientCache -ErrorAction SilentlyContinue

$restoreAt = (Get-Date).AddHours(24)
$scriptPath = $MyInvocation.MyCommand.Path
$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$scriptPath`" -Restore"
$trigger = New-ScheduledTaskTrigger -Once -At $restoreAt
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries

Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Settings $settings -RunLevel Highest -Force | Out-Null

Write-Host ''
Write-Host '[OK] Wi-Fi (Waffle) priority is active for 24 hours!' -ForegroundColor Green
Write-Host '     Waffle   IPv4 metric = 5   (AutomaticMetric = Disabled)'
Write-Host '     Ethernet IPv4 metric = 500 (AutomaticMetric = Disabled)'
Write-Host '     Ethernet IPv6        = Disabled for 24h'
Write-Host '     WcmSvc MinimizeConn  = 0 (Wi-Fi stays active with Ethernet)'
Write-Host ("[INFO] Auto-restore scheduled at: {0}" -f $restoreAt.ToString('yyyy-MM-dd HH:mm:ss')) -ForegroundColor Cyan
Write-Host ''
