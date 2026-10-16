# Rebuilds the app icons in assets/app-icon from the two SVG sources.
#
#   sift-mail.svg         macOS (824px tile on a 1024 canvas, Apple's grid)
#   sift-mail-square.svg  Windows and Linux (the tile nearly fills the canvas)
#
# Microsoft Edge renders each SVG at 1024px; System.Drawing scales it down and
# this script packs the sizes into SiftMail.ico (Windows) and SiftMail.icns
# (macOS). Run from the repository root on Windows:
#
#   powershell -ExecutionPolicy Bypass -File tools/make-icons.ps1

param(
    [string]$Edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

$out = Resolve-Path "assets/app-icon"
$work = Join-Path ([IO.Path]::GetTempPath()) "sift-icons"
New-Item -ItemType Directory -Force $work | Out-Null

function Render([string]$name) {
    $svg = (Join-Path $out "$name.svg") -replace '\\', '/'
    $html = Join-Path $work "$name.html"
    "<html><body style='margin:0'><img src='file:///$svg' width='1024' height='1024' style='display:block'></body></html>" |
        Set-Content -Encoding utf8 $html
    $png = Join-Path $work "$name-1024.png"
    Remove-Item -ErrorAction SilentlyContinue $png
    Start-Process -Wait -WindowStyle Hidden -FilePath $Edge -ArgumentList @(
        "--headless=new", "--disable-gpu", "--hide-scrollbars",
        "--default-background-color=00000000", "--force-device-scale-factor=1",
        "--window-size=1024,1024", "--screenshot=`"$png`"", "`"file:///$html`"")
    if (-not (Test-Path $png)) { throw "Edge didn't render $name.svg" }
    $png
}

function Scale([string]$master, [int]$size) {
    $src = [Drawing.Bitmap]::FromFile($master)
    $dst = New-Object Drawing.Bitmap $size, $size, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [Drawing.Graphics]::FromImage($dst)
    $g.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.CompositingQuality = [Drawing.Drawing2D.CompositingQuality]::HighQuality
    $g.DrawImage($src, 0, 0, $size, $size)
    $g.Dispose(); $src.Dispose()
    $ms = New-Object IO.MemoryStream
    $dst.Save($ms, [Drawing.Imaging.ImageFormat]::Png)
    $dst.Dispose()
    , $ms.ToArray()
}

function BigEndian([int]$n) {
    $b = [BitConverter]::GetBytes([uint32]$n); [Array]::Reverse($b); $b
}

$square = Render "sift-mail-square"
$mac = Render "sift-mail"

# PNGs for Linux and the installers.
$pngDir = Join-Path $out "png"
New-Item -ItemType Directory -Force $pngDir | Out-Null
foreach ($size in 16, 24, 32, 48, 64, 128, 256, 512, 1024) {
    [IO.File]::WriteAllBytes((Join-Path $pngDir "$size.png"), (Scale $square $size))
}

# Windows .ico: PNG-compressed entries, largest last.
$sizes = 16, 20, 24, 32, 40, 48, 64, 256
$images = $sizes | ForEach-Object { , (Scale $square $_) }
$ico = New-Object IO.MemoryStream
$w = New-Object IO.BinaryWriter $ico
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]; $len = $images[$i].Length
    $w.Write([byte]($s % 256)); $w.Write([byte]($s % 256))
    $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$len); $w.Write([uint32]$offset)
    $offset += $len
}
foreach ($img in $images) { $w.Write($img) }
$w.Flush()
[IO.File]::WriteAllBytes((Join-Path $out "SiftMail.ico"), $ico.ToArray())

# macOS .icns: PNG entries by type code.
$types = [ordered]@{ "icp4" = 16; "icp5" = 32; "ic11" = 32; "ic12" = 64; "ic07" = 128;
    "ic08" = 256; "ic13" = 512; "ic09" = 512; "ic14" = 1024; "ic10" = 1024 }
$body = New-Object IO.MemoryStream
foreach ($t in $types.Keys) {
    $img = Scale $mac $types[$t]
    $body.Write([Text.Encoding]::ASCII.GetBytes($t), 0, 4)
    $body.Write((BigEndian ($img.Length + 8)), 0, 4)
    $body.Write($img, 0, $img.Length)
}
$icns = New-Object IO.MemoryStream
$icns.Write([Text.Encoding]::ASCII.GetBytes("icns"), 0, 4)
$icns.Write((BigEndian ($body.Length + 8)), 0, 4)
$body.WriteTo($icns)
[IO.File]::WriteAllBytes((Join-Path $out "SiftMail.icns"), $icns.ToArray())

"Wrote $out"
