<#
.SYNOPSIS
    MeNote 快捷启动脚本：启动本地服务并自动打开管理后台
.DESCRIPTION
    默认在独立窗口前台运行 node server.js（实时看日志，关窗即停服）。
    -Silent 改为后台静默运行，日志写入 %TEMP%\menote-server.log。
    若 3107 端口已有实例在跑：默认直接复用并打开浏览器；
    -Restart 先停旧实例再启新（改过 .htm 模板后必须重启，jj.js 生产模式缓存编译模板）。
.PARAMETER Restart
    端口被占用时先停掉旧实例再启动
.PARAMETER Silent
    后台静默运行，日志写入 %TEMP%\menote-server.log
.PARAMETER NoBrowser
    启动成功后不自动打开浏览器
.EXAMPLE
    .\start.ps1              # 前台窗口启动 + 打开浏览器
    .\start.ps1 -Restart      # 重启（改了模板后用这个）
    .\start.ps1 -Silent       # 后台静默启动
    .\start.ps1 -NoBrowser    # 启动但不弹浏览器
#>
param(
    [switch]$Restart,
    [switch]$Silent,
    [switch]$NoBrowser
)

$ErrorActionPreference = 'Stop'

# 端口与 server.js 中硬编码的 3107 保持一致；脚本放在项目根目录，路径自适应
$Port     = 3107
$Root     = $PSScriptRoot
$Url      = "http://localhost:$Port"
$AdminUrl = "$Url/admin"
$Log      = Join-Path $env:TEMP 'menote-server.log'

function Test-Listening {
    return [bool](Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue)
}

function Get-ListenPid {
    $c = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1
    if($c) { return $c.OwningProcess }
    return $null
}

function Wait-HttpReady([int]$TimeoutSec = 30) {
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while((Get-Date) -lt $deadline) {
        try {
            $r = Invoke-WebRequest -Uri $Url -UseBasicParsing -TimeoutSec 2
            if($r.StatusCode -eq 200) { return $true }
        } catch {
            Start-Sleep -Milliseconds 500
        }
    }
    return $false
}

Write-Host '==> MeNote 快捷启动' -ForegroundColor Cyan
Write-Host "    项目目录: $Root"

# 1) node 环境检查
if(-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Write-Host '[错误] 未找到 node，请先安装 Node.js' -ForegroundColor Red
    exit 1
}

# 2) 已有实例在跑 → 复用，或按 -Restart 重启
if(Test-Listening) {
    $listenPid = Get-ListenPid
    if($Restart) {
        $proc = Get-Process -Id $listenPid -ErrorAction SilentlyContinue
        # 安全阀：只杀 node（MeNote），端口被别的程序占用时提示手动处理
        if($proc -and $proc.ProcessName -ne 'node') {
            Write-Host "[错误] 端口 $Port 被 $($proc.ProcessName) (PID $listenPid) 占用，不是 MeNote，请手动处理" -ForegroundColor Red
            exit 1
        }
        Write-Host "==> -Restart：停止旧实例 (PID $listenPid)..." -ForegroundColor Yellow
        Stop-Process -Id $listenPid -Force -ErrorAction SilentlyContinue
        $deadline = (Get-Date).AddSeconds(10)
        while((Test-Listening) -and ((Get-Date) -lt $deadline)) {
            Start-Sleep -Milliseconds 300
        }
        if(Test-Listening) {
            Write-Host "[错误] 旧实例停止失败，请手动结束 PID $listenPid 后重试" -ForegroundColor Red
            exit 1
        }
    } else {
        Write-Host "==> 服务已在运行 (PID $listenPid)，直接复用" -ForegroundColor Green
        if(-not $NoBrowser) { Start-Process $AdminUrl }
        Write-Host "    管理后台: $AdminUrl"
        Write-Host '    如需重启（改过模板后必须重启）: .\start.ps1 -Restart'
        exit 0
    }
}

# 3) 启动服务
if($Silent) {
    # 后台静默：WMI Create 脱离当前终端，关掉本窗口也不影响运行
    Write-Host "==> 静默启动，日志: $Log"
    $cmd = 'cmd.exe /c "cd /d "{0}" && node server.js > "{1}" 2>&1"' -f $Root, $Log
    Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = $cmd } | Out-Null
} else {
    # 前台窗口：独立控制台实时显示日志，关闭该窗口即停止服务
    Write-Host '==> 在新窗口前台启动（关闭该窗口即停止服务；要后台运行用 -Silent）'
    $inner = "chcp 65001 >`$null; Set-Location -LiteralPath '$Root'; " +
        "Write-Host '==> MeNote 运行中，关闭此窗口即停止服务' -ForegroundColor Cyan; node server.js"
    Start-Process -FilePath 'powershell.exe' -ArgumentList @('-NoExit', '-Command', $inner) -WindowStyle Normal
}

# 4) 等待就绪
Write-Host '==> 等待服务就绪（最多 30 秒）...'
if(-not (Wait-HttpReady 30)) {
    Write-Host '[错误] 服务未能在 30 秒内就绪' -ForegroundColor Red
    if($Silent -and (Test-Path $Log)) {
        Write-Host '---- 日志尾部 ----' -ForegroundColor Yellow
        Get-Content $Log -Tail 20
    }
    exit 1
}

# 5) 完成
Write-Host "==> 启动成功 (PID $(Get-ListenPid))" -ForegroundColor Green
Write-Host "    前台首页: $Url"
Write-Host "    管理后台: $AdminUrl   （admin123 / admin123）"
if(-not $NoBrowser) { Start-Process $AdminUrl }
