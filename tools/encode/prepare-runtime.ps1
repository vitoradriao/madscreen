<#
  Prepara o runtime FFmpeg/x264 do madscreen: baixa o pacote fixado em runtime-gpl.json (versao e SHA-256 fixos),
  extrai e, com -BundleDestination, copia so as DLLs e os avisos que acompanham o instalador.
  O pacote e produzido por build-native-runtime-gpl.ps1 + package-runtime-gpl.ps1 (veja docs/runtime-distribution.md).
  -ArchivePath usa um zip local (por exemplo, recem-empacotado) em vez de baixar; o hash continua sendo conferido.
#>
param(
    [string]$Destination = "$PSScriptRoot/../../target/encoder-runtime",
    [string]$BundleDestination,
    [string]$ArchivePath
)
$ErrorActionPreference = 'Stop'
$manifest = Get-Content -LiteralPath "$PSScriptRoot/runtime-gpl.json" -Raw | ConvertFrom-Json
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
$Destination = (Resolve-Path -LiteralPath $Destination).Path
$archive = Join-Path $Destination $manifest.archiveName
if ($ArchivePath) { Copy-Item -LiteralPath $ArchivePath -Destination $archive -Force }
elseif (!(Test-Path -LiteralPath $archive)) { Invoke-WebRequest $manifest.url -OutFile $archive }
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $manifest.sha256) {
    throw 'Checksum do runtime invalido. Remova o arquivo baixado e tente novamente.'
}
$runtime = Join-Path $Destination $manifest.name
if (!(Test-Path -LiteralPath (Join-Path $runtime 'bin/avcodec-62.dll'))) {
    Expand-Archive -LiteralPath $archive -DestinationPath $runtime -Force
}
if (!(Test-Path -LiteralPath (Join-Path $runtime 'bin/avcodec-62.dll'))) {
    throw 'Pacote sem a ABI esperada (avcodec-62.dll).'
}
if ($BundleDestination) {
    New-Item -ItemType Directory -Force -Path $BundleDestination | Out-Null
    foreach ($dll in $manifest.bundleDlls) { Copy-Item -LiteralPath "$runtime/bin/$dll" -Destination $BundleDestination -Force }
    # Licenca, oferta de codigo-fonte e avisos acompanham as bibliotecas redistribuidas.
    Copy-Item -LiteralPath "$runtime/LICENSE" -Destination $BundleDestination -Force
    Copy-Item -LiteralPath "$runtime/README.txt" -Destination $BundleDestination -Force
    Copy-Item -LiteralPath "$runtime/notices/SOURCE-OFFER.md" -Destination $BundleDestination -Force
    Copy-Item -LiteralPath "$runtime/notices/x264-GPL-2.0.txt" -Destination $BundleDestination -Force
    Copy-Item -LiteralPath "$PSScriptRoot/NOTICE.md" -Destination $BundleDestination -Force
}
Write-Output "Runtime pronto: $runtime/bin"
