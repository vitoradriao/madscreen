<#
  Empacota o runtime GPL minimo (saida de build-native-runtime-gpl.ps1) para publicacao como asset de release e
  junta as fontes GPL correspondentes (FFmpeg e x264). Nao publica nada: so cria arquivos numa pasta nova.
  Uso: pwsh -File tools/encode/package-runtime-gpl.ps1 -Runtime <repo>\target\runtime-gpl-4 -Output <repo>\target\runtime-gpl-release -Label 2026-11-04
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Runtime,
    [Parameter(Mandatory = $true)][string]$Output,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9]{4}-[0-9]{2}-[0-9]{2}(\.[0-9]+)?$')][string]$Label,
    [string]$SourceRoot = "$PSScriptRoot/../../target/runtime-native-final",
    [string]$ArchiveCache = "$PSScriptRoot/../../target/runtime-gpl-cache"
)
$ErrorActionPreference = 'Stop'
$resolve = { param($p) $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($p) }
$rt = & $resolve $Runtime; $out = & $resolve $Output; $src = & $resolve $SourceRoot; $cache = & $resolve $ArchiveCache
if (Test-Path -LiteralPath $out) { throw 'Saida existe; use uma pasta nova.' }
$bin = "$rt/runtime/bin"; $notices = "$rt/runtime/notices"
$dlls = 'avcodec-62.dll', 'avutil-60.dll', 'swresample-6.dll', 'libx264-165.dll', 'libvpl.dll'
# ffmpeg.exe e as DLLs que ele usa entram so para auditoria/diagnostico do pacote; o instalador leva as cinco acima.
$tools = 'ffmpeg.exe', 'ffprobe.exe', 'avformat-62.dll', 'avfilter-11.dll', 'avdevice-62.dll', 'swscale-9.dll'
foreach ($name in $dlls + $tools) { if (-not (Test-Path -LiteralPath "$bin/$name")) { throw "Ausente no runtime: $name" } }
foreach ($name in 'SOURCE-OFFER.md', 'FFmpeg-GPL-2.0.txt', 'x264-GPL-2.0.txt', 'config.mak') { if (-not (Test-Path -LiteralPath "$notices/$name")) { throw "Aviso ausente: $name" } }
$stage = "$out/stage"
New-Item -ItemType Directory -Path "$stage/bin", "$stage/notices", "$out/sources" -Force | Out-Null
foreach ($name in $dlls + $tools) { Copy-Item -LiteralPath "$bin/$name" -Destination "$stage/bin/$name" }
Copy-Item -Path "$notices/*" -Destination "$stage/notices" -Recurse
# Nomes que o inventario do instalador espera.
Copy-Item -LiteralPath "$notices/FFmpeg-GPL-2.0.txt" -Destination "$stage/LICENSE"
Copy-Item -LiteralPath "$notices/SOURCE-OFFER.md" -Destination "$stage/README.txt"
$zip = "$out/madscreen-runtime-gpl-$Label.zip"
# Ordem e datas fixas para o hash nao depender do relogio.
$items = Get-ChildItem -LiteralPath $stage -Recurse -File | Sort-Object FullName
foreach ($item in $items) { $item.LastWriteTime = [datetime]'2026-01-01T00:00:00Z' }
Compress-Archive -Path "$stage/*" -DestinationPath $zip -CompressionLevel Optimal
# Fontes GPL correspondentes (hash ja conferido no build).
Copy-Item -LiteralPath "$src/archives/ffmpeg.tar.gz" -Destination "$out/sources/FFmpeg-760d10603e.tar.gz"
Copy-Item -LiteralPath "$cache/x264.tar.gz" -Destination "$out/sources/x264-b35605ace3ddf7c1a5d67a2eb553f034aef41d55.tar.gz"
Copy-Item -LiteralPath "$rt/sources-gpl.json" -Destination "$out/sources/sources-gpl.json"
Copy-Item -LiteralPath "$src/sources.json" -Destination "$out/sources/sources-base.json"
$sums = foreach ($file in @(Get-Item $zip) + @(Get-ChildItem "$out/sources" -File)) {
    "$((Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant())  $($file.Name)"
}
$sums | Set-Content -LiteralPath "$out/SHA256SUMS.txt" -Encoding ascii
Write-Output ($sums -join "`n")
