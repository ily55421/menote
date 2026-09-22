# 从 PNG 源生成多尺寸 Windows 图标（.ico）
#
# 用途：MeNote.exe 的文件图标（资源管理器/任务栏）与窗口图标。
# 尺寸覆盖 Windows 常用档位：16/24/32/48/64/128/256，
# 缺了大尺寸会导致"大图标"视图模糊、任务栏高清屏发虚。
#
# 实现说明：手工拼装 ICO 容器（ICONDIR + ICONDIRENTRY* + PNG 数据）。
# Vista 起 ICO 允许直接内嵌 PNG（256x256 必须用 PNG，BMP 有 256 限制且体积大），
# 因此这里对所有尺寸统一用 PNG 编码，简单且无损。
param(
    [string]$Source = '',
    [string]$Output = ''
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$Root = Split-Path $PSScriptRoot -Parent
if(-not $Source) { $Source = Join-Path $Root 'fpk\ICON_256.PNG' }
if(-not $Output) { $Output = Join-Path $PSScriptRoot 'launcher\assets\menote.ico' }

if(-not (Test-Path $Source)) { throw "源图片不存在：$Source" }

$sizes = @(16, 24, 32, 48, 64, 128, 256)
$src = [System.Drawing.Image]::FromFile((Resolve-Path $Source).Path)
Write-Host "源图: $Source  ($($src.Width)x$($src.Height))"

# 1) 各尺寸缩放到内存 PNG
$pngs = @()
foreach($s in $sizes) {
    $bmp = New-Object System.Drawing.Bitmap($s, $s, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    # 高质量缩放：HighQualityBicubic 对图标的小尺寸缩略效果最好
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)
    $g.DrawImage($src, 0, 0, $s, $s)
    $g.Dispose()

    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    $pngs += ,@{ size = $s; bytes = $ms.ToArray() }
    $ms.Dispose()
}
$src.Dispose()

# 2) 拼装 ICO
$outDir = Split-Path $Output -Parent
if(-not (Test-Path $outDir)) { New-Item -ItemType Directory -Force -Path $outDir | Out-Null }

$fs = [System.IO.File]::Create($Output)
$bw = New-Object System.IO.BinaryWriter($fs)
try {
    # ICONDIR：保留(2) + 类型(2, 1=图标) + 图像数(2)
    $bw.Write([uint16]0)
    $bw.Write([uint16]1)
    $bw.Write([uint16]$pngs.Count)

    # ICONDIRENTRY 固定 16 字节，数据区紧跟在目录之后
    $offset = 6 + 16 * $pngs.Count
    foreach($p in $pngs) {
        # 256 在 ICO 目录里用 0 表示（单字节存不下）
        $dim = if($p.size -ge 256) { 0 } else { $p.size }
        $bw.Write([byte]$dim)              # 宽
        $bw.Write([byte]$dim)              # 高
        $bw.Write([byte]0)                 # 调色板数（PNG 无调色板）
        $bw.Write([byte]0)                 # 保留
        $bw.Write([uint16]1)               # 颜色平面
        $bw.Write([uint16]32)              # 位深
        $bw.Write([uint32]$p.bytes.Length) # 数据大小
        $bw.Write([uint32]$offset)         # 数据偏移
        $offset += $p.bytes.Length
    }
    foreach($p in $pngs) { $bw.Write($p.bytes) }
    $bw.Flush()
} finally {
    $bw.Dispose(); $fs.Dispose()
}

$out = Get-Item $Output
Write-Host ("生成: {0}  ({1:N1} KB, {2} 个尺寸: {3})" -f $out.FullName, ($out.Length/1KB), $pngs.Count, ($sizes -join '/'))

# 3) 另存 256x256 PNG 供窗口图标使用
#    tao 的 Icon::from_rgba 只吃原始像素，build.rs 会把它解码成 RGBA 内嵌。
#    单独出 PNG 是为了让 ICO（多尺寸，含小尺寸优化）与窗口图标各取所需。
$winPng = Join-Path (Split-Path $Output -Parent) 'icon-256.png'
$bmp256 = New-Object System.Drawing.Bitmap(256, 256, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g256 = [System.Drawing.Graphics]::FromImage($bmp256)
$g256.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g256.Clear([System.Drawing.Color]::Transparent)
$src2 = [System.Drawing.Image]::FromFile((Resolve-Path $Source).Path)
$g256.DrawImage($src2, 0, 0, 256, 256)
$g256.Dispose(); $src2.Dispose()
$bmp256.Save($winPng, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp256.Dispose()
Write-Host ("窗口图标: {0}  ({1:N1} KB)" -f $winPng, ((Get-Item $winPng).Length/1KB))
