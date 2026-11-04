$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$windowsDir = Join-Path $repoRoot 'apps/madscreen-app/src-tauri/windows'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$visualStudio = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $visualStudio) { throw 'MSVC x86/x64 Build Tools nao encontrado.' }
$compiler = Get-ChildItem -Path (Join-Path $visualStudio 'VC/Tools/MSVC/*/bin/Hostx64/x86/cl.exe') | Sort-Object FullName -Descending | Select-Object -First 1
$vcvars = Join-Path $visualStudio 'VC/Auxiliary/Build/vcvars32.bat'
$output = Join-Path $repoRoot 'bench-results/installer-theme-build'
New-Item -ItemType Directory -Force $output | Out-Null
$dll = Join-Path $windowsDir 'branding/madscreen-theme.dll'
$source = Join-Path $windowsDir 'theme_sys.cpp'
# NSIS runs an x86 process even when it installs an x64 application.
$build = Join-Path $output 'build.cmd'
@"
@echo off
call "$vcvars" >nul
if errorlevel 1 exit /b %errorlevel%
"$($compiler.FullName)" /nologo /std:c++17 /W4 /WX /EHsc /MT /O2 /LD /D_WIN32_WINNT=0x0601 "$source" /Fo"$output/theme.obj" /link /OUT:"$dll" /IMPLIB:"$output/theme.lib" user32.lib gdi32.lib comctl32.lib dwmapi.lib gdiplus.lib uxtheme.lib
"@ | Set-Content -LiteralPath $build -Encoding ascii
& $env:ComSpec /d /c $build
if ($LASTEXITCODE -ne 0) { throw 'Falha compilando tema NSIS.' }
