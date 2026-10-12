<#
  Costruisce l'installer di ArchLine (MSI, WiX 3) da un clone del repository.

  Requisiti: Rust (MSVC), ImageMagick (`magick`, per le icone dei file DWG/DXF),
  WiX Toolset 3.x (candle.exe, light.exe, WixUIExtension.dll).

  Uso:
    pwsh packaging\archline-windows\build-msi.ps1 [-Wix <cartella WiX>] [-Out dist] [-SkipBuild]

  -Wix        cartella con candle.exe/light.exe, o la sua radice con `bin\` (default: $env:WIX)
  -Out        cartella di destinazione dell'MSI (default: dist)
  -SkipBuild  non esegue cargo: usa target\release\ArchLine.exe e dwg_thumbnailer_win.dll già costruiti

  L'MSI non è firmato: Windows SmartScreen mostra un avviso al primo avvio.
#>
param(
    [string]$Wix = $env:WIX,
    [string]$Out = "dist",
    [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Set-Location $root

# Versione dal Cargo.toml: AAAA.S.P -> MSI (AA).S.P (l'MSI vuole major <= 255).
$cargoVersion = (Select-String -Path Cargo.toml -Pattern '^version = "(\d+)\.(\d+)\.(\d+)"' |
    Select-Object -First 1).Matches[0]
if (-not $cargoVersion) { throw "versione non trovata in Cargo.toml" }
$year, $minor, $patch = [int]$cargoVersion.Groups[1].Value, [int]$cargoVersion.Groups[2].Value, [int]$cargoVersion.Groups[3].Value
$msiVersion = "{0}.{1}.{2}" -f ($year - 2000), $minor, $patch
if (($year - 2000) -gt 255 -or $minor -gt 255 -or $patch -gt 65535) { throw "versione MSI fuori limite: $msiVersion" }
$version = "{0}.{1}.{2}" -f $year, $minor, $patch

# Strumenti WiX.
if (-not $Wix) { throw "WiX non trovato: passa -Wix <cartella> o imposta WIX" }
$bin = if (Test-Path (Join-Path $Wix "bin\candle.exe")) { Join-Path $Wix "bin" } else { $Wix }
$candle = Join-Path $bin "candle.exe"
$light = Join-Path $bin "light.exe"
if (-not (Test-Path $candle) -or -not (Test-Path $light)) { throw "candle.exe/light.exe non trovati in $bin" }

# Icone dei file DWG/DXF dai SVG di assets (come fa la release di upstream).
$work = Join-Path $root "target\msi"
New-Item -ItemType Directory -Force $work | Out-Null
foreach ($kind in "dwg", "dxf") {
    magick "assets/mimetypes/image-vnd.$kind.svg" -define icon:auto-resize=16,24,32,48,64,128,256 (Join-Path $work "$kind.ico")
    if ($LASTEXITCODE -ne 0) { throw "magick fallito per $kind.ico" }
}

if (-not $SkipBuild) {
    cargo build --locked --release --bin ArchLine
    if ($LASTEXITCODE -ne 0) { throw "cargo build fallito" }
    cargo build --locked --release -p dwg-thumbnailer-win
    if ($LASTEXITCODE -ne 0) { throw "cargo build (thumbnailer) fallito" }
}
$exe = (Resolve-Path "target\release\ArchLine.exe").Path
$thumb = (Resolve-Path "target\release\dwg_thumbnailer_win.dll").Path
$icon = (Resolve-Path "packaging\windows\AppIcon.ico").Path
$license = (Resolve-Path "packaging\windows\License.rtf").Path
$src = Join-Path $root "packaging\archline-windows"

$obj = Join-Path $work "obj\"
New-Item -ItemType Directory -Force $obj | Out-Null
& $candle -nologo -arch x64 "-dVersion=$msiVersion" "-dSource=$exe" "-dIcon=$icon" "-dIconsDir=$work" `
    "-dThumbDll=$thumb" "-dLicense=$license" (Join-Path $src "main.wxs") (Join-Path $src "ui.wxs") -out $obj
if ($LASTEXITCODE -ne 0) { throw "candle fallito" }

New-Item -ItemType Directory -Force $Out | Out-Null
$msi = Join-Path (Resolve-Path $Out).Path "ArchLine-$version-windows-x64.msi"
& $light -nologo (Join-Path $obj "main.wixobj") (Join-Path $obj "ui.wixobj") -ext WixUIExtension `
    -cultures:it-it -loc (Join-Path $src "strings.it-IT.wxl") -out $msi
if ($LASTEXITCODE -ne 0) { throw "light fallito" }
Write-Host "MSI: $msi"
