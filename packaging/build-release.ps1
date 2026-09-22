<#
.SYNOPSIS
    构建 MeNote 单文件发行版（MeNote.exe）
.DESCRIPTION
    流程：
      1. cargo build --release 生成裸启动器
      2. 组装 staging：node.exe + 项目全量文件（排除开发/文档/数据目录）
      3. 压缩为 payload.zip
      4. 拼接：裸启动器 + payload.zip + version + ver_len + zip_len + magic
    产物：packaging\build\MeNote.exe（单文件，双击即用）

    数据不打包：data\ 与 config\lock.js 必须排除——
    data\ 是用户数据，lock.js 是"已安装"标记（打进去会让新用户跳过安装向导）。
    二者由启动器在运行时按需创建。
.PARAMETER Version
    覆盖版本号（默认取 package.json 的 version）
.PARAMETER SkipBuild
    跳过 cargo build（复用上次的裸启动器，仅重打包 payload）
.EXAMPLE
    .\build-release.ps1
    .\build-release.ps1 -Version 1.1.0
#>
param(
    [string]$Version,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$ScriptDir  = $PSScriptRoot
$ProjectRoot = Split-Path $ScriptDir -Parent          # 仓库根
$LauncherDir = Join-Path $ScriptDir 'launcher'
$BuildDir    = Join-Path $ScriptDir 'build'
$StagingDir  = Join-Path $BuildDir 'staging'
$PayloadZip  = Join-Path $BuildDir 'payload.zip'
$BareExe     = Join-Path $LauncherDir 'target\release\MeNote.exe'
$OutExe      = Join-Path $BuildDir 'MeNote.exe'

# 尾部格式，必须与 launcher/src/main.rs 的常量保持一致
$MAGIC = [byte[]]@(0x4D,0x45,0x4E,0x4F,0x54,0x45,0x5F,0x50,0x4B,0x47,0x5F,0x76,0x31,0x00,0x00,0x00)  # "MENOTE_PKG_v1\0\0\0"

function Write-Step([string]$text) {
    Write-Host "==> $text" -ForegroundColor Cyan
}

# 顶层排除项：开发工具链、文档、平台专用目录、数据目录
$ExcludeTop = @(
    '.git', '.github', '.vscode',
    'android', 'docker', 'docs', 'fpk', 'packaging', 'Temp',
    'soft',              # 发行版入口目录：内含 exe 自身，绝不能打包进去
    'node_modules',      # 单独处理（体积大，用 robocopy 复制）
    'data',              # 用户数据：绝不打包
    'start.ps1', 'start.bat', 'docker-compose.yml', 'pelican-bike.html'
)

Write-Step '检查构建环境'
if(-not (Get-Command node -ErrorAction SilentlyContinue)) { throw '未找到 node' }
if(-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw '未找到 cargo（需安装 Rust 工具链）' }
if(-not (Test-Path (Join-Path $ProjectRoot 'package.json'))) { throw "未找到 package.json：$ProjectRoot" }

# 版本号
if(-not $Version) {
    $pkgRaw = Get-Content (Join-Path $ProjectRoot 'package.json') -Raw
    if($pkgRaw -match '"version"\s*:\s*"([^"]+)"') { $Version = $Matches[1] }
    else { throw 'package.json 缺少 version 字段' }
}
Write-Host "    版本: $Version"

# ---- 1) 编译启动器 ----
if(-not $SkipBuild) {
    Write-Step '编译 Rust 启动器（release）'
    Push-Location $LauncherDir
    try {
        # 把版本号经环境变量传给 build.rs：exe 资源节的版本信息要跟实际发布版本一致。
        # 否则 -Version 1.3.0 构建出的文件属性里仍显示 package.json 的版本（1.0.0）。
        $env:MENOTE_BUILD_VERSION = $Version
        & cargo build --release
        if($LASTEXITCODE -ne 0) { throw "cargo build 失败（退出码 $LASTEXITCODE）" }
    } finally {
        Remove-Item Env:MENOTE_BUILD_VERSION -ErrorAction SilentlyContinue
        Pop-Location
    }
} else {
    Write-Step '跳过编译，复用已有启动器'
}
if(-not (Test-Path $BareExe)) { throw "未找到启动器：$BareExe" }
Write-Host ("    启动器: {0:N2} MB" -f ((Get-Item $BareExe).Length / 1MB))

# ---- 2) 组装 staging ----
Write-Step '组装运行环境（staging）'
if(Test-Path $StagingDir) { Remove-Item $StagingDir -Recurse -Force }
New-Item -ItemType Directory -Path $StagingDir -Force | Out-Null

# 2a) node.exe（发行版自带运行时，用户无需装 Node）
Copy-Item (Get-Command node).Source (Join-Path $StagingDir 'node.exe') -Force

# 2b) 项目文件（robocopy：/E 含子目录 /XD 排除目录 /XF 排除文件）
$xdArgs = @()
foreach($d in $ExcludeTop) { $xdArgs += @($d, (Join-Path $ProjectRoot $d)) }
$xfArgs = @('pelican-bike.html', 'start.ps1', 'start.bat', 'docker-compose.yml')

# 项目根（不含 node_modules）
$rcArgs = @($ProjectRoot, $StagingDir, '/E', '/NFL', '/NDL', '/NJH', '/NJS', '/NP', '/R:1', '/W:1')
$rcArgs += '/XD'; $rcArgs += $xdArgs
$rcArgs += '/XF'; $rcArgs += $xfArgs
& robocopy @rcArgs | Out-Null
# robocopy 退出码 0-7 表示成功（8+ 才是错误）
if($LASTEXITCODE -ge 8) { throw "robocopy 复制项目文件失败（退出码 $LASTEXITCODE）" }

# 2c) node_modules（生产依赖全量：含 sqlite3 / iroh 原生模块）
$nmSrc = Join-Path $ProjectRoot 'node_modules'
$nmDst = Join-Path $StagingDir 'node_modules'
& robocopy $nmSrc $nmDst /E /NFL /NDL /NJH /NJS /NP /R:1 /W:1 `
    /XD (Join-Path $nmSrc '.cache') | Out-Null
if($LASTEXITCODE -ge 8) { throw "robocopy 复制 node_modules 失败（退出码 $LASTEXITCODE）" }

# 2d) 清理：安装标记与用户数据绝不能进包
Remove-Item (Join-Path $StagingDir 'config\lock.js') -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $StagingDir 'data') -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $StagingDir 'Temp') -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $StagingDir 'public\upload') -Recurse -Force -ErrorAction SilentlyContinue

# 校验关键文件
foreach($must in @('node.exe', 'server.js', 'package.json', 'config\db.js', 'app\admin\view\index_index.htm')) {
    $p = Join-Path $StagingDir $must
    if(-not (Test-Path $p)) { throw "staging 缺少关键文件：$must" }
}
if(Test-Path (Join-Path $StagingDir 'config\lock.js')) { throw 'staging 残留 config/lock.js（会导致新用户跳过安装向导）' }
if(Test-Path (Join-Path $StagingDir 'data')) { throw 'staging 残留 data 目录（不应打包用户数据）' }

$stageSize = (Get-ChildItem $StagingDir -Recurse -File | Measure-Object Length -Sum).Sum
Write-Host ("    已组装: {0:N1} MB" -f ($stageSize / 1MB))

# ---- 3) 压缩 payload ----
Write-Step '压缩 payload.zip'
if(Test-Path $PayloadZip) { Remove-Item $PayloadZip -Force }
Add-Type -AssemblyName System.IO.Compression.FileSystem
# 用 ZipFile 而非 Compress-Archive：更快，且对深层 node_modules 更稳
[System.IO.Compression.ZipFile]::CreateFromDirectory(
    $StagingDir, $PayloadZip,
    [System.IO.Compression.CompressionLevel]::Optimal,
    $false   # includeBaseDirectory=false：包内为相对路径
)
$zipSize = (Get-Item $PayloadZip).Length
Write-Host ("    payload.zip: {0:N1} MB（压缩率 {1:P0}）" -f ($zipSize / 1MB), (1 - $zipSize / $stageSize))

# ---- 4) 拼接单文件 exe ----
Write-Step '拼接单文件发行版'
$bareBytes = [System.IO.File]::ReadAllBytes($BareExe)
$zipBytes  = [System.IO.File]::ReadAllBytes($PayloadZip)
$verBytes  = [System.Text.Encoding]::UTF8.GetBytes($Version)

# 产物被运行中的实例锁定是常见情况（用户正开着 MeNote）。
# 提前探测并给出明确指引，而不是抛出难懂的 IOException
if(Test-Path $OutExe) {
    try {
        $probe = [System.IO.File]::Open($OutExe, 'Open', 'ReadWrite', 'None')
        $probe.Dispose()
    } catch {
        throw "产物被占用，无法覆盖：$OutExe`n请先退出正在运行的 MeNote（或结束 MeNote.exe / node 进程）后重试。"
    }
}

$fs = [System.IO.File]::Create($OutExe)
try {
    $bw = New-Object System.IO.BinaryWriter($fs)
    $bw.Write($bareBytes)                                            # 裸启动器
    $bw.Write($zipBytes)                                             # payload.zip
    $bw.Write($verBytes)                                             # version（UTF-8）
    $bw.Write([uint32]$verBytes.Length)                              # ver_len (u32 LE)
    $bw.Write([uint64]$zipBytes.Length)                              # zip_len (u64 LE)
    $bw.Write($MAGIC)                                                # magic (16B)
    $bw.Flush()
} finally { $fs.Dispose() }

$outSize = (Get-Item $OutExe).Length
$expected = $bareBytes.Length + $zipBytes.Length + $verBytes.Length + 4 + 8 + 16
if($outSize -ne $expected) { throw "产物大小异常：$outSize != $expected" }

# ---- 5) 发布到使用入口目录 soft/ ----
# soft/ 是日常使用的正式入口，构建后自动同步过去（覆盖旧版即完成升级）
$SoftDir = Join-Path $ProjectRoot 'soft'
$SoftExe = Join-Path $SoftDir 'MeNote.exe'
$published = $false
if(Test-Path $SoftDir) {
    Write-Step '发布到使用入口 soft\'
    try {
        $probe = [System.IO.File]::Open($SoftExe, 'Open', 'ReadWrite', 'None')
        $probe.Dispose()
    } catch {
        Write-Host '    跳过：soft\MeNote.exe 正被占用（请先退出运行中的 MeNote）' -ForegroundColor Yellow
        $published = $null
    }
    if($published -ne $null) {
        Copy-Item $OutExe $SoftExe -Force
        $published = $true
        Write-Host ("    已更新: {0}" -f $SoftExe)
    }
} else {
    $published = $null
}

Write-Host ''
Write-Host '构建完成' -ForegroundColor Green
Write-Host ("    产物: {0}" -f $OutExe)
Write-Host ("    版本: {0}" -f $Version)
Write-Host ("    大小: {0:N1} MB" -f ($outSize / 1MB))
if($published -eq $true) { Write-Host ("    使用入口: {0}（已同步）" -f $SoftExe) }
elseif($published -eq $null) { Write-Host '    使用入口: soft\ 未同步（见上方提示）' -ForegroundColor Yellow }
Write-Host ''
Write-Host '运行说明：' -ForegroundColor Yellow
Write-Host '  - 日常使用: 双击 soft\MeNote.exe'
Write-Host '  - 首次/更新后启动约 6~10 秒释放运行环境，之后秒开'
Write-Host '  - 用户数据: %APPDATA%\MeNote\data（更新版本不会丢失）'
Write-Host '  - 程序文件: %LOCALAPPDATA%\MeNote\runtime'
Write-Host '  - 日志:     %LOCALAPPDATA%\MeNote\{launcher,server}.log'
Write-Host '  - 版本更新: 退出 MeNote 后重新构建，或手动覆盖 soft\MeNote.exe'
