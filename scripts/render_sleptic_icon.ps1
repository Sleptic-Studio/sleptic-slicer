# Copyright (c) 2026 Sleptic
# SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
# Offline renderer for the rounded-rectangle Sleptic SVG mark. Rejects other geometry.
param([string]$Source = "$PSScriptRoot/../apps/slicer-gui/assets/images/sleptic.svg", [string]$Destination = "$PSScriptRoot/../apps/slicer-gui/assets/icons/sleptic.ico")
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
[xml]$svg = Get-Content -LiteralPath $Source -Raw -Encoding UTF8
$ns = New-Object System.Xml.XmlNamespaceManager($svg.NameTable)
$ns.AddNamespace('s', 'http://www.w3.org/2000/svg')
$rects = $svg.SelectNodes('/s:svg/s:g/s:rect', $ns)
if ($rects.Count -ne 5 -or $svg.SelectNodes('/s:svg/s:g/*', $ns).Count -ne 5) { throw 'Unsupported SVG geometry; update the offline renderer.' }
$stops = $svg.SelectNodes('/s:svg/s:defs/s:linearGradient/s:stop', $ns)
$top = [Drawing.ColorTranslator]::FromHtml($stops[0].GetAttribute('stop-color'))
$bottom = [Drawing.ColorTranslator]::FromHtml($stops[1].GetAttribute('stop-color'))
$frames = @()
foreach ($size in @(16, 24, 32, 48, 64, 128, 256)) {
    $large = New-Object Drawing.Bitmap ($size*4), ($size*4)
    $graphics = [Drawing.Graphics]::FromImage($large)
    $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.ScaleTransform($size*4/128.0, $size*4/128.0)
    foreach ($r in $rects) {
        [single]$x = $r.x; [single]$y = $r.y; [single]$w = $r.width; [single]$h = $r.height; [single]$diameter = 2*[single]$r.rx
        $path = New-Object Drawing.Drawing2D.GraphicsPath
        $path.AddArc($x, $y, $diameter, $diameter, 180, 90)
        $path.AddArc($x+$w-$diameter, $y, $diameter, $diameter, 270, 90)
        $path.AddArc($x+$w-$diameter, $y+$h-$diameter, $diameter, $diameter, 0, 90)
        $path.AddArc($x, $y+$h-$diameter, $diameter, $diameter, 90, 90)
        $path.CloseFigure()
        $bounds = New-Object Drawing.RectangleF $x, $y, $w, $h
        $brush = New-Object Drawing.Drawing2D.LinearGradientBrush $bounds, $top, $bottom, ([Drawing.Drawing2D.LinearGradientMode]::Vertical)
        $graphics.FillPath($brush, $path)
        $brush.Dispose(); $path.Dispose()
    }
    $graphics.Dispose()
    $small = New-Object Drawing.Bitmap $size, $size
    $graphics = [Drawing.Graphics]::FromImage($small)
    $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.DrawImage($large, 0, 0, $size, $size)
    $stream = New-Object IO.MemoryStream
    $small.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
    $frames += ,@($size, $stream.ToArray())
    $stream.Dispose(); $graphics.Dispose(); $small.Dispose(); $large.Dispose()
}
$output = [IO.File]::Create([IO.Path]::GetFullPath($Destination))
$writer = New-Object IO.BinaryWriter $output
try {
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$frames.Count)
    [uint32]$offset = 6 + 16*$frames.Count
    foreach ($frame in $frames) {
        [byte]$dim = if ($frame[0] -eq 256) { 0 } else { $frame[0] }
        $writer.Write($dim); $writer.Write($dim); $writer.Write([byte]0); $writer.Write([byte]0)
        $writer.Write([uint16]1); $writer.Write([uint16]32); $writer.Write([uint32]$frame[1].Length); $writer.Write($offset)
        $offset += $frame[1].Length
    }
    foreach ($frame in $frames) { $writer.Write([byte[]]$frame[1]) }
} finally { $writer.Dispose() }
Write-Output "Rendered $Destination"
