<# Candidato LGPL isolado. Nao promove bundle, instala ou altera runtime existente. #>
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Destination,
    [string]$SourceCache,
    [switch]$ResumeBuild
)
$ErrorActionPreference='Stop'
$root=$ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Destination)
# FFmpeg/GNU Make nativo exigem paths sem espacos neste builder. Nao e limite do app.
if($root -notmatch '^[A-Za-z]:\\[A-Za-z0-9_\\.-]+$'){throw 'Use destino absoluto sem espacos/metacaracteres.'}
$manifest=Get-Content -LiteralPath "$PSScriptRoot/native-runtime-sources.json" -Raw | ConvertFrom-Json
if(Test-Path -LiteralPath $root){
    if(-not $ResumeBuild){throw 'Destino existe; use outro ou -ResumeBuild no proprio build validado.'}
    $previous=Get-Content -LiteralPath "$root/sources.json" -Raw | ConvertFrom-Json
    if(($previous | ConvertTo-Json -Depth 8 -Compress) -ne ($manifest | ConvertTo-Json -Depth 8 -Compress)){throw 'Manifesto do destino difere; retomada recusada.'}
}elseif($ResumeBuild){throw 'Nao existe build para retomar.'}
$vswhere="${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
$vs=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if(-not $vs){throw 'MSVC Build Tools nao encontrado.'}
$gitBash='C:/Program Files/Git/bin/bash.exe'
$gitShell='C:/Progra~1/Git/usr/bin/sh.exe'
if(-not (Test-Path -LiteralPath $gitBash) -or -not (Test-Path -LiteralPath $gitShell)){throw 'Git for Windows no local esperado e caminho curto sao necessarios.'}
New-Item -ItemType Directory -Path $root -Force | Out-Null
New-Item -ItemType Directory -Path "$root/archives","$root/logs" -Force | Out-Null
foreach($source in $manifest.sources){
    $archive="$root/archives/$($source.name).tar.gz"
    if(-not ($ResumeBuild -and (Test-Path -LiteralPath $archive))){
        if($SourceCache){Copy-Item -LiteralPath (Join-Path $SourceCache "$($source.name).tar.gz") -Destination $archive}
        else{Invoke-WebRequest -Uri $source.url -OutFile $archive}
    }
    if((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $source.sha256){throw "Hash divergente: $($source.name)"}
    $members=@()
    if($source.name -eq 'meson'){$members=@("$($source.root)/meson.py","$($source.root)/mesonbuild")}
    if($source.name -eq 'amf'){$members=@("$($source.root)/amf/public/include","$($source.root)/LICENSE.txt")}
    & tar -xzf $archive -C $root @members
    if($LASTEXITCODE -ne 0){throw "Extracao falhou: $($source.name)"}
}
Copy-Item -LiteralPath "$PSScriptRoot/native-runtime-sources.json" -Destination "$root/sources.json"
$native=$root.Replace('\','/')
$unix='/'+$native.Substring(0,1).ToLowerInvariant()+$native.Substring(2)
$ffmpeg=($manifest.sources | Where-Object name -eq 'ffmpeg').root
$vpl=($manifest.sources | Where-Object name -eq 'vpl').root
$amf=($manifest.sources | Where-Object name -eq 'amf').root
$meson=($manifest.sources | Where-Object name -eq 'meson').root
$pkgconf=($manifest.sources | Where-Object name -eq 'pkgconf').root
$make=($manifest.sources | Where-Object name -eq 'make').root
# VERSION evita que version.sh herde o Git do aplicativo pai; nenhuma alteracao C.
Set-Content -LiteralPath "$root/$ffmpeg/VERSION" -Value $manifest.ffmpegVersion -Encoding ascii
New-Item -ItemType Directory -Path "$root/prefix/include","$root/ffmpeg-build" -Force | Out-Null
if(-not (Test-Path -LiteralPath "$root/prefix/include/AMF/core")){
    Copy-Item -LiteralPath "$root/$amf/amf/public/include" -Destination "$root/prefix/include/AMF" -Recurse
}
@'
import sys
for line in sys.stdin:
    if 'including file:' in line:
        path=line.split('including file:',1)[1].strip().replace(chr(92),'/')
        if ' ' not in path:
            print(sys.argv[1]+': '+path)
'@ | Set-Content -LiteralPath "$root/msvc-deps.py" -Encoding utf8
@'
import sys
from pathlib import Path
root=Path(sys.argv[1])
native=str(root).replace(chr(92),'/')
msys='/'+native[0].lower()+native[2:]
for name in ['Makefile','ffbuild/config.mak']:
    path=root/'ffmpeg-build'/name
    value=path.read_text().replace(msys,native)
    lines=[]
    for line in value.splitlines():
        if line.startswith(('CCDEP=','CXXDEP=','ASDEP=','HOSTCCDEP=')):
            line=line.split(' | awk ',1)[0]+' | python '+native+'/msvc-deps.py $@ > $(@:.o=.d)'
        lines.append(line)
    path.write_text('\n'.join(lines)+'\n')
'@ | Set-Content -LiteralPath "$root/normalize-build.py" -Encoding utf8
$configure=@"
#!/usr/bin/env bash
set -euo pipefail
export PATH="$unix/tools/bin:`$PATH"
export PKG_CONFIG_LIBDIR='$native/prefix/lib/pkgconfig'
cd '$native/ffmpeg-build'
../$ffmpeg/configure --toolchain=msvc --arch=x86_64 --target-os=win32 --prefix=$native/runtime --disable-autodetect --disable-everything --disable-x86asm --disable-static --enable-shared --disable-gpl --disable-nonfree --disable-version3 --enable-amf --enable-libvpl --enable-mediafoundation --enable-d3d11va --enable-w32threads --enable-swresample --enable-swscale --enable-encoder=h264_amf,h264_qsv,h264_mf,rawvideo,pcm_s16le --enable-decoder=h264,rawvideo --enable-parser=h264 --enable-muxer=h264,null --enable-demuxer=h264,rawvideo --enable-protocol=file,pipe --enable-filter=testsrc2,format,scale,null --enable-indev=lavfi --pkg-config=pkgconf --extra-cflags=-I$native/prefix/include --disable-doc
"@
# Bash nao interpreta BOM ou CRLF como cabecalho/comando.
$utf8=New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllText("$root/configure.sh",$configure.Replace("`r`n","`n"),$utf8)
$build=@"
#!/usr/bin/env bash
set -euo pipefail
export PATH="$unix/$make/WinRel:`$PATH"
cd '$native/ffmpeg-build'
gnumake.exe SHELL=$gitShell -j4
gnumake.exe SHELL=$gitShell V=1 install-libs install-progs install-data
"@
[IO.File]::WriteAllText("$root/build.sh",$build.Replace("`r`n","`n"),$utf8)
$cmake="$vs/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin"
$ninja="$vs/Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja"
$driver=@"
@echo off
call "$vs/VC/Auxiliary/Build/vcvars64.bat"
if errorlevel 1 exit /b 1
set VSLANG=1033
set "PATH=$cmake;$ninja;%PATH%"
cd /d "$root/$make"
call build_w32.bat --without-guile
if errorlevel 1 exit /b 1
python "$root/$meson/meson.py" setup "$root/pkgconf-build" "$root/$pkgconf" --prefix "$root/tools" --buildtype release -Dtests=disabled -Ddefault_library=static
if errorlevel 1 exit /b 1
python "$root/$meson/meson.py" compile -C "$root/pkgconf-build"
if errorlevel 1 exit /b 1
python "$root/$meson/meson.py" install -C "$root/pkgconf-build"
if errorlevel 1 exit /b 1
cmake -S "$root/$vpl" -B "$root/vpl-build" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$root/prefix" -DBUILD_DISPATCHER=ON -DBUILD_DEV=ON -DBUILD_PREVIEW=OFF -DBUILD_TOOLS=OFF -DBUILD_TOOLS_ONEVPL_EXPERIMENTAL=OFF -DINSTALL_EXAMPLE_CODE=OFF -DBUILD_SHARED_LIBS=ON -DBUILD_TESTS=OFF -DUSE_MSVC_STATIC_RUNTIME=ON "-DCMAKE_C_FLAGS_RELEASE=/MT /O2 /Ob2 /DNDEBUG" "-DCMAKE_CXX_FLAGS_RELEASE=/MT /O2 /Ob2 /DNDEBUG"
if errorlevel 1 exit /b 1
cmake --build "$root/vpl-build" --parallel 4
if errorlevel 1 exit /b 1
cmake --install "$root/vpl-build"
if errorlevel 1 exit /b 1
"$gitBash" "$native/configure.sh"
if errorlevel 1 exit /b 1
python "$root/normalize-build.py" "$root"
if errorlevel 1 exit /b 1
"$gitBash" "$native/build.sh"
exit /b %ERRORLEVEL%
"@
Set-Content -LiteralPath "$root/build.cmd" -Value $driver -Encoding ascii
& $env:ComSpec /d /c "$root/build.cmd" *> "$root/logs/build.log"
if($LASTEXITCODE -ne 0){Get-Content "$root/logs/build.log" -Tail 30;throw 'Build falhou; evidencia preservada.'}
Copy-Item -LiteralPath "$root/prefix/bin/libvpl.dll" -Destination "$root/runtime/bin/libvpl.dll"
& python "$PSScriptRoot/install-native-headers.py" $root
if($LASTEXITCODE -ne 0){throw 'Instalacao dos headers incompleta.'}
New-Item -ItemType Directory -Path "$root/runtime/notices" -Force | Out-Null
Copy-Item -LiteralPath "$root/$ffmpeg/COPYING.LGPLv2.1" -Destination "$root/runtime/notices/FFmpeg-LGPL-2.1.txt"
Copy-Item -LiteralPath "$root/$vpl/LICENSE" -Destination "$root/runtime/notices/oneVPL-MIT.txt"
Copy-Item -LiteralPath "$root/$amf/LICENSE.txt" -Destination "$root/runtime/notices/AMF.txt"
Copy-Item -LiteralPath "$root/ffmpeg-build/ffbuild/config.mak" -Destination "$root/runtime/notices/config.mak"
Copy-Item -LiteralPath "$root/sources.json" -Destination "$root/runtime/notices/sources.json"
$files=Get-ChildItem -LiteralPath "$root/runtime" -Recurse -File | ForEach-Object {
    [ordered]@{path=$_.FullName.Substring($root.Length+1);bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
}
[ordered]@{schema=1;distributionApproved=$false;toolchain=$vs;files=@($files)} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$root/inventory.json" -Encoding utf8
Write-Output "Build local concluido, sem promocao: $root"
