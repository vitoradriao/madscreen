param(
    [string]$Destination = "$PSScriptRoot/../../target/encoder-runtime",
    [string]$BundleDestination
)
$ErrorActionPreference = 'Stop'
# ABI fixa: libavcodec 62 / libavutil 60. Nao usar builds nightly.
$version = '8.1.2'
$checksum = 'cba748035c21ce1431d0823c7a3a711f38616f89f87a265dceddf9b7f6749d2d'
$archiveName = "ffmpeg-$version-full_build-shared.7z"
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
$Destination = (Resolve-Path -LiteralPath $Destination).Path
$archive = Join-Path $Destination $archiveName
if (!(Test-Path -LiteralPath $archive)) {
    Invoke-WebRequest "https://www.gyan.dev/ffmpeg/builds/packages/$archiveName" -OutFile $archive
}
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $checksum) {
    throw 'Checksum do runtime FFmpeg invalido. Remova o arquivo baixado e tente novamente.'
}
$runtime = Join-Path $Destination "ffmpeg-$version-full_build-shared"
if (!(Test-Path -LiteralPath (Join-Path $runtime 'bin/avcodec-62.dll'))) {
    & tar -xf $archive -C $Destination
    if ($LASTEXITCODE -ne 0) { throw 'Falha ao extrair o runtime FFmpeg.' }
}
if (!(Test-Path -LiteralPath (Join-Path $runtime 'bin/avcodec-62.dll'))) {
    throw 'Pacote FFmpeg sem a ABI esperada (avcodec-62.dll).'
}
if ($BundleDestination) {
    New-Item -ItemType Directory -Force -Path $BundleDestination | Out-Null
    # Dependencias PE de avcodec: avutil e swresample. Nao distribuir filtros/players.
    foreach ($dll in @('avcodec-62.dll', 'avutil-60.dll', 'swresample-6.dll')) {
        Copy-Item -LiteralPath "$runtime/bin/$dll" -Destination $BundleDestination -Force
    }
    # Licencas e configuracao da build acompanham as bibliotecas redistribuidas.
    Copy-Item -LiteralPath "$runtime/LICENSE" -Destination $BundleDestination -Force
    Copy-Item -LiteralPath "$runtime/README.txt" -Destination $BundleDestination -Force
    Copy-Item -LiteralPath "$PSScriptRoot/NOTICE.md" -Destination $BundleDestination -Force
}
Write-Output "Runtime pronto: $runtime/bin"
