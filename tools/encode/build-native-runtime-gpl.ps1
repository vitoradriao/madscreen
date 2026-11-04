<#
  Runtime GPL MINIMO do madscreen: FFmpeg 760d10603e + x264 (fixados por hash) + AMF + oneVPL, compilados com MSVC.
  Substitui o pacote "full_build" de terceiros, cujas DLLs embutem dezenas de bibliotecas que o app nao usa.
  Aplicativo: GPL-3.0-or-later. O resultado leva `notices/SOURCE-OFFER.md` com as fontes e hashes exatos: e a
  oferta de codigo-fonte correspondente (veja docs/runtime-distribution.md).

  Duas etapas: (1) `build-native-runtime.ps1` prepara a base (fonte do FFmpeg, make, pkgconf, oneVPL, AMF);
  (2) este script compila o x264 e refaz o FFmpeg com `--enable-gpl --enable-libx264`, FORA da arvore, em pasta nova.
  A base em -SourceRoot so e LIDA. Modos: sem switches = plano; -GenerateOnly = gera os scripts e para;
  -Execute = baixa/valida o x264 e o NASM, compila (BelowNormal, -j1 por padrao).
  Uso:
    pwsh -File tools/encode/build-native-runtime-gpl.ps1 -Destination <repo>\target\runtime-gpl
    pwsh -File tools/encode/build-native-runtime-gpl.ps1 -Destination <repo>\target\runtime-gpl -Execute
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Destination,
    [string]$SourceRoot = "$PSScriptRoot/../../target/runtime-native-final",
    # Pasta com x264.tar.gz e nasm.zip ja baixados (conferidos por SHA-256); sem ela o script baixa dos enderecos do manifesto.
    [string]$ArchiveCache,
    [ValidateRange(1, 4)][int]$Jobs = 1,
    [switch]$Execute,
    [switch]$GenerateOnly
)
$ErrorActionPreference = 'Stop'
if ($Execute -and $GenerateOnly) { throw 'Use -Execute ou -GenerateOnly, nao ambos.' }
$resolve = { param($p) $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($p) }
$root = & $resolve $Destination
$src = & $resolve $SourceRoot
foreach ($pair in @(@('Destination', $root), @('SourceRoot', $src))) {
    if ($pair[1] -notmatch '^[A-Za-z]:\\[A-Za-z0-9_\\.-]+$') { throw "$($pair[0]): use caminho absoluto sem espacos/metacaracteres." }
}
$x = $root.TrimEnd('\').ToLowerInvariant(); $y = $src.TrimEnd('\').ToLowerInvariant()
if ($x -eq $y -or $x.StartsWith($y + '\') -or $y.StartsWith($x + '\')) { throw 'Destination deve ficar fora de (e nao conter) SourceRoot.' }
if (Test-Path -LiteralPath $root) { throw 'Destino existe; use uma pasta nova (a tentativa anterior fica preservada).' }
$manifest = Get-Content -LiteralPath "$PSScriptRoot/native-runtime-gpl-sources.json" -Raw | ConvertFrom-Json
$x264 = $manifest.sources | Where-Object name -eq 'x264'
$nasm = $manifest.sources | Where-Object name -eq 'nasm'
$ffmpeg = 'FFmpeg-760d10603e'
$required = @{
    ffmpegConfigure = "$src/$ffmpeg/configure"; gnumake = "$src/make-4.4.1/WinRel/gnumake.exe"; pkgconf = "$src/tools/bin/pkgconf.exe"
    msvcDeps = "$src/msvc-deps.py"; normalize = "$src/normalize-build.py"; vplPc = "$src/prefix/lib/pkgconfig/vpl.pc"
    ffmpegGpl = "$src/$ffmpeg/COPYING.GPLv2"; sourcesJson = "$src/sources.json"
}
foreach ($entry in $required.GetEnumerator()) { if (-not (Test-Path -LiteralPath $entry.Value)) { throw "Base ausente ($($entry.Key)): $($entry.Value). Rode build-native-runtime.ps1 antes." } }
$native = $root.Replace('\', '/')
$srcNative = $src.Replace('\', '/')
$srcUnix = '/' + $srcNative.Substring(0, 1).ToLowerInvariant() + $srcNative.Substring(2)
$unix = '/' + $native.Substring(0, 1).ToLowerInvariant() + $native.Substring(2)
$flagList = @(
    '--toolchain=msvc', '--arch=x86_64', '--target-os=win32', "--prefix=$native/runtime",
    '--disable-autodetect', '--disable-everything', '--disable-x86asm', '--disable-static', '--enable-shared',
    # GPL (por causa do x264); sem nonfree.
    '--enable-gpl', '--disable-nonfree', '--disable-version3',
    '--enable-amf', '--enable-libvpl', '--enable-mediafoundation', '--enable-d3d11va', '--enable-w32threads',
    '--enable-swresample', '--enable-swscale', '--enable-libx264',
    '--enable-encoder=h264_amf,h264_qsv,h264_mf,libx264,rawvideo,pcm_s16le',
    '--enable-decoder=h264,rawvideo', '--enable-parser=h264', '--enable-muxer=h264,null',
    '--enable-demuxer=h264,rawvideo', '--enable-protocol=file,pipe',
    '--enable-filter=testsrc2,format,scale,null', '--enable-indev=lavfi', '--pkg-config=pkgconf',
    "--extra-cflags=-I$srcNative/prefix/include -I$native/prefix/include",
    "--extra-ldflags=-LIBPATH:$srcNative/prefix/lib -LIBPATH:$native/prefix/lib",
    '--disable-doc'
)
$bashQuote = { param($text) "'" + $text.Replace("'", "'\''") + "'" }
$configureArguments = ($flagList | ForEach-Object { & $bashQuote $_ }) -join ' '
$plan = [ordered]@{
    destination = $root; base = $src; x264 = $x264.url; x264Sha256 = $x264.sha256; nasmSha256 = $nasm.sha256
    jobs = $Jobs; priority = 'BelowNormal'; ffmpegConfigure = $flagList
    notes = 'Runtime GPL minimo (FFmpeg + x264 + AMF + oneVPL). Resultado: runtime/bin e runtime/notices/SOURCE-OFFER.md.'
}
if (-not $Execute -and -not $GenerateOnly) { $plan | ConvertTo-Json -Depth 4; Write-Output 'Plano apenas; nada foi criado. Use -GenerateOnly (scripts) ou -Execute (build).'; return }

[System.Diagnostics.Process]::GetCurrentProcess().PriorityClass = 'BelowNormal'
$vswhere = "${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'MSVC Build Tools nao encontrado.' }
$gitBash = 'C:/Program Files/Git/bin/bash.exe'
$gitShell = 'C:/Progra~1/Git/usr/bin/sh.exe'
if (-not (Test-Path -LiteralPath $gitBash) -or -not (Test-Path -LiteralPath $gitShell)) { throw 'Git for Windows no local esperado e necessario.' }
New-Item -ItemType Directory -Path "$root/archives", "$root/logs", "$root/prefix/include", "$root/prefix/lib/pkgconfig", "$root/ffmpeg-build" -Force | Out-Null

foreach ($source in @($x264, $nasm)) {
    $archive = "$root/archives/$($source.name)" + $(if ($source.url -like '*.zip') { '.zip' } else { '.tar.gz' })
    if (-not $Execute) { continue }
    if ($ArchiveCache) {
        $cached = Join-Path (& $resolve $ArchiveCache) (Split-Path $archive -Leaf)
        if (-not (Test-Path -LiteralPath $cached)) { throw "Cache sem $(Split-Path $archive -Leaf)" }
        Copy-Item -LiteralPath $cached -Destination $archive
    } else { Invoke-WebRequest -Uri $source.url -OutFile $archive }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $source.sha256) { throw "Hash divergente: $($source.name)" }
    if ($archive.EndsWith('.zip')) { Expand-Archive -LiteralPath $archive -DestinationPath $root }
    else { & "$env:windir/System32/tar.exe" -xzf $archive -C $root; if ($LASTEXITCODE -ne 0) { throw "Extracao falhou: $($source.name)" } }
}
Copy-Item -LiteralPath "$PSScriptRoot/native-runtime-gpl-sources.json" -Destination "$root/sources-gpl.json"
Copy-Item -LiteralPath $required.msvcDeps -Destination "$root/msvc-deps.py"
Copy-Item -LiteralPath $required.normalize -Destination "$root/normalize-build.py"
@'
import sys
from pathlib import Path
root = Path(sys.argv[1])
old, new = sys.argv[2], sys.argv[3]
for name in ['Makefile', 'ffbuild/config.mak']:
    path = root / 'ffmpeg-build' / name
    path.write_text(path.read_text().replace(old, new))
'@ | Set-Content -LiteralPath "$root/normalize-source.py" -Encoding utf8
$utf8 = New-Object System.Text.UTF8Encoding($false)
$nasmUnix = "$unix/$($nasm.root)"
$x264Script = @"
#!/usr/bin/env bash
set -euo pipefail
export PATH='$srcUnix/make-4.4.1/WinRel':'$nasmUnix':"`$PATH"
cd '$native/$($x264.root)'
CC=cl ./configure --prefix='$native/prefix' --enable-shared --disable-cli --disable-avs --disable-swscale --disable-lavf --disable-ffms --disable-gpac --disable-lsmash
gnumake.exe SHELL=$gitShell -j$Jobs
gnumake.exe SHELL=$gitShell install-lib-dev install-lib-shared
# O wrapper mslink do FFmpeg converte -lx264 em libx264.lib; o x264 instala a import lib como libx264.dll.lib.
for name in x264.lib libx264.lib; do
  if [ ! -f "$native/prefix/lib/`$name" ]; then cp '$native/prefix/lib/libx264.dll.lib' "$native/prefix/lib/`$name"; fi
done
test -f '$native/prefix/lib/libx264.lib'
"@
[IO.File]::WriteAllText("$root/x264.sh", $x264Script.Replace("`r`n", "`n"), $utf8)
$configure = @"
#!/usr/bin/env bash
set -euo pipefail
export PATH='$srcUnix/tools/bin':"`$PATH"
export PKG_CONFIG_LIBDIR='$native/prefix/lib/pkgconfig;$srcNative/prefix/lib/pkgconfig'
cd '$native/ffmpeg-build'
'$srcNative/$ffmpeg/configure' $configureArguments
"@
[IO.File]::WriteAllText("$root/configure.sh", $configure.Replace("`r`n", "`n"), $utf8)
$build = @"
#!/usr/bin/env bash
set -euo pipefail
export PATH='$srcUnix/make-4.4.1/WinRel':"`$PATH"
cd '$native/ffmpeg-build'
gnumake.exe SHELL=$gitShell -j$Jobs
gnumake.exe SHELL=$gitShell V=1 install-libs install-progs install-data
"@
[IO.File]::WriteAllText("$root/build.sh", $build.Replace("`r`n", "`n"), $utf8)
$driver = @"
@echo off
call "$vs/VC/Auxiliary/Build/vcvars64.bat"
if errorlevel 1 exit /b 1
set VSLANG=1033
"$gitBash" "$native/x264.sh"
if errorlevel 1 exit /b 1
"$gitBash" "$native/configure.sh"
if errorlevel 1 exit /b 1
python "$root/normalize-build.py" "$root"
if errorlevel 1 exit /b 1
python "$root/normalize-source.py" "$root" "$srcUnix" "$srcNative"
if errorlevel 1 exit /b 1
"$gitBash" "$native/build.sh"
exit /b %ERRORLEVEL%
"@
Set-Content -LiteralPath "$root/build.cmd" -Value $driver -Encoding ascii
$plan | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath "$root/plan.json" -Encoding utf8
if ($GenerateOnly) { Write-Output "Scripts gerados, sem build: $root"; return }
& $env:ComSpec /d /c "$root/build.cmd" *> "$root/logs/build.log"
if ($LASTEXITCODE -ne 0) { Get-Content "$root/logs/build.log" -Tail 30; throw 'Build falhou; evidencia preservada.' }

# DLLs: libvpl (externo) + libx264; depois conferir dependencias e escrever avisos/oferta de fonte.
Copy-Item -LiteralPath "$src/runtime/bin/libvpl.dll" -Destination "$root/runtime/bin/libvpl.dll"
$x264Dll = Get-ChildItem "$root/prefix/bin" -Filter 'libx264-*.dll' | Select-Object -First 1
if (-not $x264Dll) { throw 'libx264-*.dll nao encontrada em prefix/bin.' }
Copy-Item -LiteralPath $x264Dll.FullName -Destination "$root/runtime/bin/$($x264Dll.Name)"
$dumpbin = Get-ChildItem "$vs/VC/Tools/MSVC/*/bin/Hostx64/x64/dumpbin.exe" | Select-Object -Last 1 -ExpandProperty FullName
$system = Join-Path $env:windir 'System32'
$dependencies = foreach ($dll in Get-ChildItem -LiteralPath "$root/runtime/bin" -Filter '*.dll') {
    foreach ($name in (& $dumpbin /nologo /dependents $dll.FullName | Select-String -Pattern '^\s+\S+\.dll\s*$' | ForEach-Object { $_.Line.Trim() })) {
        $local = Test-Path -LiteralPath (Join-Path "$root/runtime/bin" $name)
        $isSystem = Test-Path -LiteralPath (Join-Path $system $name)
        if (-not $local -and -not $isSystem) { throw "Dependencia ausente de $($dll.Name): $name" }
        [ordered]@{ dll = $dll.Name; dependsOn = $name; source = $(if ($local) { 'runtime/bin' } else { 'System32' }) }
    }
}
$dependencies | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath "$root/dependencies.json" -Encoding utf8
New-Item -ItemType Directory -Path "$root/runtime/notices" -Force | Out-Null
Copy-Item -LiteralPath $required.ffmpegGpl -Destination "$root/runtime/notices/FFmpeg-GPL-2.0.txt"
Copy-Item -LiteralPath "$src/$ffmpeg/COPYING.LGPLv2.1" -Destination "$root/runtime/notices/FFmpeg-LGPL-2.1.txt"
Copy-Item -LiteralPath "$root/$($x264.root)/COPYING" -Destination "$root/runtime/notices/x264-GPL-2.0.txt"
Copy-Item -LiteralPath $required.sourcesJson -Destination "$root/runtime/notices/sources-base.json"
Copy-Item -LiteralPath "$root/sources-gpl.json" -Destination "$root/runtime/notices/sources-gpl.json"
Copy-Item -LiteralPath "$root/ffmpeg-build/ffbuild/config.mak" -Destination "$root/runtime/notices/config.mak"
$base = Get-Content -LiteralPath $required.sourcesJson -Raw | ConvertFrom-Json
$offer = @("# Oferta de codigo-fonte correspondente", "",
  "Este runtime (DLLs do FFmpeg e do x264) foi compilado a partir das fontes abaixo, fixadas por SHA-256, com os scripts",
  "``tools/encode/build-native-runtime.ps1`` e ``tools/encode/build-native-runtime-gpl.ps1`` do repositorio do madscreen",
  "(GPL-3.0-or-later). Quem recebe o instalador pode obter as mesmas fontes, sem custo, nos enderecos abaixo e",
  "tambem no repositorio de releases, onde cada release publica uma copia dos arquivos.", "",
  "| Componente | Licenca | Fonte | SHA-256 |", "| --- | --- | --- | --- |")
foreach ($s in $base.sources) { if ($s.name -in 'ffmpeg', 'amf', 'vpl') { $offer += "| $($s.name) | ver arquivo de licenca | $($s.url) | $($s.sha256) |" } }
foreach ($s in @($x264)) { $offer += "| $($s.name) | $($s.license) | $($s.url) | $($s.sha256) |" }
$offer += @("", "Configuracao do FFmpeg: ``notices/config.mak`` (linha FFMPEG_CONFIGURATION).",
  "Nao ha patches no FFmpeg nem no x264. Ferramentas de build (make, pkgconf, meson, NASM) nao fazem parte do pacote.")
$offer -join "`n" | Set-Content -LiteralPath "$root/runtime/notices/SOURCE-OFFER.md" -Encoding utf8
$files = Get-ChildItem -LiteralPath "$root/runtime" -Recurse -File | ForEach-Object {
    [ordered]@{ path = $_.FullName.Substring($root.Length + 1); bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
}
[ordered]@{ schema = 1; license = 'GPL-3.0-or-later (aplicativo); FFmpeg/x264 GPL'; files = @($files) } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$root/inventory.json" -Encoding utf8
Write-Output "Runtime GPL minimo concluido: $root"
