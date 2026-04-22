# Generate placeholder brand icons for Tauri build.
# Creates a stark black square with a white "L" glyph (Loreglide wordmark).
Add-Type -AssemblyName System.Drawing

$iconsDir = Join-Path $PSScriptRoot "..\src-tauri\icons"
if (-not (Test-Path $iconsDir)) { New-Item -ItemType Directory -Path $iconsDir -Force | Out-Null }

function New-BrandBitmap {
    param([int]$Size)
    $bmp = New-Object System.Drawing.Bitmap($Size, $Size)
    $gfx = [System.Drawing.Graphics]::FromImage($bmp)
    $gfx.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $gfx.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
    $gfx.Clear([System.Drawing.Color]::Black)

    $fontSize = [Math]::Max([int]($Size * 0.72), 8)
    $font = New-Object System.Drawing.Font("Consolas", $fontSize, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
    $brush = [System.Drawing.Brushes]::White

    $sf = New-Object System.Drawing.StringFormat
    $sf.Alignment = [System.Drawing.StringAlignment]::Center
    $sf.LineAlignment = [System.Drawing.StringAlignment]::Center

    $rect = New-Object System.Drawing.RectangleF(0, 0, $Size, $Size)
    $gfx.DrawString("L", $font, $brush, $rect, $sf)

    $font.Dispose()
    $sf.Dispose()
    $gfx.Dispose()
    return $bmp
}

$sizes = @{
    "32x32.png"        = 32
    "128x128.png"      = 128
    "128x128@2x.png"   = 256
    "icon.png"         = 512
}

foreach ($name in $sizes.Keys) {
    $bmp = New-BrandBitmap -Size $sizes[$name]
    $bmp.Save((Join-Path $iconsDir $name), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Host "  wrote $name ($($sizes[$name])x$($sizes[$name]))"
}

# --- Build icon.ico containing 16, 32, 48, 64, 128, 256 pixel variants.
$icoPath = Join-Path $iconsDir "icon.ico"
$icoSizes = 16, 32, 48, 64, 128, 256
$images = $icoSizes | ForEach-Object { New-BrandBitmap -Size $_ }

# Save each image as PNG in memory.
$pngBytes = $images | ForEach-Object {
    $ms = New-Object System.IO.MemoryStream
    $_.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    ,$ms.ToArray()
}

# Build ICO file structure manually.
# ICONDIR: reserved(2) + type(2) + count(2) = 6 bytes.
# ICONDIRENTRY: width(1) + height(1) + colorcount(1) + reserved(1) + planes(2) + bitcount(2) + size(4) + offset(4) = 16 bytes per image.
$headerSize = 6 + (16 * $icoSizes.Count)
$fs = [System.IO.File]::Open($icoPath, [System.IO.FileMode]::Create)
$bw = New-Object System.IO.BinaryWriter($fs)
$bw.Write([UInt16]0)           # reserved
$bw.Write([UInt16]1)           # type = icon
$bw.Write([UInt16]$icoSizes.Count)

$offset = $headerSize
for ($i = 0; $i -lt $icoSizes.Count; $i++) {
    $size = $icoSizes[$i]
    $data = $pngBytes[$i]
    $bw.Write([Byte]($(if ($size -ge 256) { 0 } else { $size })))  # width
    $bw.Write([Byte]($(if ($size -ge 256) { 0 } else { $size })))  # height
    $bw.Write([Byte]0)                # color palette count
    $bw.Write([Byte]0)                # reserved
    $bw.Write([UInt16]1)              # color planes
    $bw.Write([UInt16]32)             # bits per pixel
    $bw.Write([UInt32]$data.Length)   # image size
    $bw.Write([UInt32]$offset)        # image offset
    $offset += $data.Length
}

foreach ($data in $pngBytes) { $bw.Write($data) }

$bw.Flush()
$bw.Close()
$fs.Close()

foreach ($img in $images) { $img.Dispose() }
Write-Host "  wrote icon.ico ($($icoSizes.Count) sizes)"

# --- Build a minimal icon.icns (macOS). Without iconutil on Windows we emit
# --- a stub — macOS bundling just needs the file to exist for the manifest;
# --- cargo/tauri-build on Windows doesn't validate its contents.
$icnsPath = Join-Path $iconsDir "icon.icns"
$bmp256 = New-BrandBitmap -Size 256
$ms = New-Object System.IO.MemoryStream
$bmp256.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
$pngData = $ms.ToArray()
$bmp256.Dispose()

$fs = [System.IO.File]::Open($icnsPath, [System.IO.FileMode]::Create)
$bw = New-Object System.IO.BinaryWriter($fs)
# ICNS magic "icns" + file size (big-endian)
$bw.Write([byte[]](0x69, 0x63, 0x6E, 0x73))
# File size = 8 (header) + 8 (chunk header) + png len
$totalSize = 8 + 8 + $pngData.Length
$sizeBytes = [BitConverter]::GetBytes([UInt32]$totalSize)
[Array]::Reverse($sizeBytes)
$bw.Write($sizeBytes)
# Chunk: "ic08" = 256x256 PNG
$bw.Write([byte[]](0x69, 0x63, 0x30, 0x38))
$chunkSize = 8 + $pngData.Length
$chunkBytes = [BitConverter]::GetBytes([UInt32]$chunkSize)
[Array]::Reverse($chunkBytes)
$bw.Write($chunkBytes)
$bw.Write($pngData)
$bw.Flush()
$bw.Close()
$fs.Close()
Write-Host "  wrote icon.icns (256x256)"

Write-Host "`nAll icons generated in $iconsDir"
