<#
.SYNOPSIS
    Безопасное переключение сервера в XKeen Route с двойной защитой от потери интернета:
    1) ПК-обёртка с мгновенным откатом по LAN, если внешний интернет пропал.
    2) Dead-Man's Switch на самом роутере (опционально через SSH) — откат на localhost, если ПК завис/отвалился.

.EXAMPLE
    .\scripts\safe-server-switch.ps1 -RouterIp 192.168.1.1 -TargetServer "MyTestServer"
    .\scripts\safe-server-switch.ps1 -RouterIp 192.168.1.1 -TargetServer "📞 VK-туннель [БС]" -UseRouterDeadman -SshPort 222 -SshUser root
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory = $false)]
    [string]$RouterIp = "192.168.1.1",

    [Parameter(Mandatory = $true)]
    [string]$TargetServer,

    [Parameter(Mandatory = $false)]
    [int]$ApiPort = 1001,

    [Parameter(Mandatory = $false)]
    [int]$TimeoutSec = 60,

    [Parameter(Mandatory = $false)]
    [string]$ProbeUrl = "https://www.gstatic.com/generate_204",

    [Parameter(Mandatory = $false)]
    [switch]$UseRouterDeadman,

    [Parameter(Mandatory = $false)]
    [string]$SshUser = "root",

    [Parameter(Mandatory = $false)]
    [int]$SshPort = 222,

    [Parameter(Mandatory = $false)]
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$BaseUrl = "http://${RouterIp}:${ApiPort}"

function Get-ActiveServer {
    $resp = Invoke-RestMethod -Uri "${BaseUrl}/api/status" -Method Get -TimeoutSec 8
    if ($resp.active_server) { return [string]$resp.active_server }
    if ($resp.data -and $resp.data.active_server) { return [string]$resp.data.active_server }
    return ""
}

function Switch-RouterServer([string]$ServerId) {
    $body = @{ server_id = $ServerId } | ConvertTo-Json -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($body)
    return Invoke-RestMethod -Uri "${BaseUrl}/api/servers/switch" -Method Post -ContentType "application/json; charset=utf-8" -Body $bytes -TimeoutSec 15
}

function Test-InternetOk {
    for ($i = 1; $i -le 2; $i++) {
        try {
            $r = Invoke-WebRequest -Uri $ProbeUrl -Method Get -TimeoutSec 8 -UseBasicParsing
            if ($r.StatusCode -ge 200 -and $r.StatusCode -lt 400) {
                return $true
            }
        } catch {
            Start-Sleep -Seconds 2
        }
    }
    return $false
}

function Arm-RouterDeadman([string]$PrevServer) {
    if (-not $UseRouterDeadman) { return $false }
    $deadmanScript = Join-Path $PSScriptRoot "router-deadman-switch.sh"
    if (-not (Test-Path $deadmanScript)) {
        Write-Warning "Deadman script not found: $deadmanScript"
        return $false
    }
    $escapedPrev = $PrevServer.Replace("'", "'\\''")
    try {
        Get-Content -Raw -Path $deadmanScript | ssh -p $SshPort -o ConnectTimeout=5 -o BatchMode=yes "${SshUser}@${RouterIp}" "cat > /tmp/router-deadman-switch.sh && chmod +x /tmp/router-deadman-switch.sh && nohup sh /tmp/router-deadman-switch.sh '${escapedPrev}' ${TimeoutSec} ${ApiPort} >/dev/null 2>&1 &"
        return ($LASTEXITCODE -eq 0)
    } catch {
        Write-Warning "Failed to arm router Dead-Man's Switch via SSH: $_"
        return $false
    }
}

function Disarm-RouterDeadman {
    if (-not $UseRouterDeadman) { return }
    try {
        ssh -p $SshPort -o ConnectTimeout=5 -o BatchMode=yes "${SshUser}@${RouterIp}" "touch /tmp/xkeen_deadman.cancel" 2>$null | Out-Null
    } catch {
        # Игнорируем ошибку снятия, если SSH недоступен
    }
}

Write-Host "[1/5] Checking current active server on ${BaseUrl}..."
$prevServer = Get-ActiveServer
if ([string]::IsNullOrWhiteSpace($prevServer)) {
    throw "Could not determine current active_server from ${BaseUrl}/api/status"
}
Write-Host "      Current server: $prevServer"

if ($prevServer -eq $TargetServer) {
    Write-Host "[OK] Target server '$TargetServer' is already active."
    exit 0
}

if (-not $Force) {
    Write-Host "[2/5] Verifying baseline internet before switching..."
    if (-not (Test-InternetOk)) {
        throw "Baseline internet is already unreachable before switch! Aborting (use -Force to override)."
    }
}

$deadmanArmed = Arm-RouterDeadman -PrevServer $prevServer
if ($UseRouterDeadman) {
    Write-Host "[3/5] Router Dead-Man's Switch armed: $deadmanArmed (timeout ${TimeoutSec}s)"
} else {
    Write-Host "[3/5] Running with PC LAN auto-rollback protection (pass -UseRouterDeadman for SSH router timer)"
}

Write-Host "[4/5] Switching server -> '$TargetServer'..."
$null = Switch-RouterServer -ServerId $TargetServer
Start-Sleep -Seconds 3

Write-Host "[5/5] Probing internet connectivity ($ProbeUrl)..."
if (Test-InternetOk) {
    if ($deadmanArmed) { Disarm-RouterDeadman }
    Write-Host "[OK] Internet is UP on '$TargetServer'. Switch confirmed."
    exit 0
}

Write-Warning "[ROLLBACK] Internet check FAILED on '$TargetServer'! Reverting to '$prevServer' via LAN..."
try {
    $null = Switch-RouterServer -ServerId $prevServer
    Start-Sleep -Seconds 2
    if ($deadmanArmed) { Disarm-RouterDeadman }
    if (Test-InternetOk) {
        Write-Host "[ROLLBACK OK] Reverted to '$prevServer', internet restored."
    } else {
        Write-Warning "[ROLLBACK WARN] Reverted to '$prevServer', but internet probe still failing."
    }
} catch {
    Write-Error "[ROLLBACK ERROR] Failed to revert via LAN API: $_"
}
exit 1
