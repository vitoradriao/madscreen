<# Instala somente no runner ou com opt-in explicito. Nunca dot-source este arquivo para testes. #>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Installer,
    [string]$PreviousInstaller,
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$WorkDir,
    [switch]$IUnderstandThisInstalls
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Test-SmokePermission {
    param([string]$Actions, [bool]$OptIn)
    return $OptIn -or [string]::Equals($Actions, 'true', [StringComparison]::Ordinal)
}
function Get-SmokeArguments {
    param([string]$Directory, [switch]$Uninstall)
    if (-not [IO.Path]::IsPathRooted($Directory) -or $Directory -match '["\r\n]') { throw 'Diretorio NSIS invalido' }
    # NSIS: /D e _?= devem ser o ultimo argumento e NAO podem ter aspas.
    if ($Uninstall) { return "/S _?=$Directory" }
    return "/S /NS /D=$Directory"
}
function Convert-SmokeVersion {
    param([string]$Text)
    if ($Text -notmatch '^(\d+)\.(\d+)\.(\d+)(?:\.(\d+))?(?:[-+][A-Za-z0-9.-]+)?$') { throw 'Versao invalida' }
    return [version](('{0}.{1}.{2}.{3}' -f $Matches[1], $Matches[2], $Matches[3], $(if ($Matches[4]) { $Matches[4] } else { '0' })))
}
function Compare-SmokePayload {
    param([hashtable]$Expected, [hashtable]$Actual)
    $missing = @($Expected.Keys | Where-Object { -not $Actual.ContainsKey($_) })
    $different = @($Expected.Keys | Where-Object { $Actual.ContainsKey($_) -and $Expected[$_] -ne $Actual[$_] })
    $extra = @($Actual.Keys | Where-Object { $_ -ne 'uninstall.exe' -and -not $Expected.ContainsKey($_) })
    return @{ ok = ($missing.Count + $different.Count + $extra.Count -eq 0); missing = $missing; different = $different; extra = $extra }
}
function Get-SmokePayloadEvidence {
    param([hashtable]$Expected, [hashtable]$Actual, [hashtable]$Previous)
    return ,@($Expected.Keys | Sort-Object | ForEach-Object {
        $name = $_
        @{ file = $name; packagedSha256 = $Expected[$name]; installedSha256 = $Actual[$name];
            previousSha256 = $Previous[$name]; retainedPrevious = ($Previous.ContainsKey($name) -and
                $Expected[$name] -ne $Previous[$name] -and $Actual[$name] -eq $Previous[$name]) }
    })
}
function Get-SmokeErrorDetail {
    param($Record, [string]$Code, [string[]]$Roots)
    $message = [string]$Record.Exception.Message
    $message = [regex]::Replace($message, '(?i)(?:[a-z]:[\\/]|\\\\)[^\r\n"'';]+', '[path]')
    foreach ($root in @($Roots | Where-Object { $_ } | Sort-Object Length -Descending)) {
        $message = [regex]::Replace($message, [regex]::Escape($root), '[path]', 'IgnoreCase')
    }
    # Redige tambem caminhos nao conhecidos (inclusive UNC); conserva motivo/exit code.
    return @{ code = $Code; errorType = $Record.Exception.GetType().Name;
        message = $message; scriptLine = $Record.InvocationInfo.ScriptLineNumber }
}
function Get-SmokeSidecarDiagnostic {
    param([string]$Text)
    $code = 'sidecar-failed'
    if ($Text -match 'runtime FFmpeg ausente') { $code = 'runtime-missing' }
    elseif ($Text -match 'ABI FFmpeg incompat') { $code = 'runtime-abi' }
    elseif ($Text -match 'DXGI_ERROR_UNSUPPORTED|0x887A0004') { $code = 'd3d11-unavailable' }
    $hresult = $null
    if ($Text -match '0x[0-9a-fA-F]{8}') { $hresult = $Matches[0] }
    return @{ code = $code; hresult = $hresult }
}
function Get-SmokeHash {
    param([string]$Path)
    # .NET inbox: independente do PSModulePath herdado de pwsh ao chamar Windows PS5.1.
    $stream = [IO.File]::OpenRead($Path)
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose(); $stream.Dispose() }
}
function Get-SmokeContract {
    param($Config, $WindowsConfig, [string]$Template, [string]$Hooks)
    if ($Config.bundle.windows.nsis.installMode -ne 'currentUser' -or
        $Template -notmatch 'RequestExecutionLevel user' -or
        -not $Template.Contains('StrCpy $INSTDIR "$LOCALAPPDATA\${PRODUCTNAME}"') -or
        -not $Template.Contains('Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}') -or
        -not $Template.Contains('WriteRegStr SHCTX "${UNINSTKEY}" "DisplayVersion" "${VERSION}"') -or
        -not $Hooks.Contains('NSIS_HOOK_PREINSTALL') -or -not $Hooks.Contains('madscreen viewer')) { throw 'Contrato NSIS mudou; revisar o smoke' }
    foreach ($value in @($Config.productName, $Config.mainBinaryName, $Config.bundle.publisher)) {
        if ($value -notmatch '^[A-Za-z0-9._ -]+$') { throw 'Identidade NSIS invalida' }
    }
    if ($WindowsConfig.bundle.resources.'encoder-runtime/' -ne 'encoder-runtime/' -or
        @($WindowsConfig.bundle.externalBin) -notcontains 'binaries/madscreen-encode-test') { throw 'Layout do bundle mudou' }
    return @{ product = $Config.productName; main = "$($Config.mainBinaryName).exe"; mode = 'currentUser';
        uninstallKey = "Software\Microsoft\Windows\CurrentVersion\Uninstall\$($Config.productName)";
        locationKey = "Software\$($Config.bundle.publisher)\$($Config.productName)" }
}

# A recusa precede leitura de executaveis, registro e Start-Process.
if (-not (Test-SmokePermission $env:GITHUB_ACTIONS $IUnderstandThisInstalls.IsPresent)) {
    $refusal = '{"schema":1,"ok":false,"steps":[{"stage":"safety","ok":false,"detail":"Recusado: exige GITHUB_ACTIONS=true ou -IUnderstandThisInstalls"}]}'
    # Unica escrita permitida na recusa: relatorio em diretorio NOVO, sem sobrescrever nada.
    try {
        $refusalRoot = [IO.Path]::GetFullPath($WorkDir)
        if (-not (Test-Path -LiteralPath $refusalRoot)) {
            New-Item -ItemType Directory -Path $refusalRoot | Out-Null
            Set-Content -LiteralPath (Join-Path $refusalRoot 'result.json') -Value $refusal -Encoding UTF8
        }
    } catch { } # Se nem o destino e valido, JSON continua disponivel em stdout.
    Write-Output $refusal
    exit 2
}

$result = [ordered]@{ schema = 1; ok = $false; upgrade = 'sem-upgrade'; steps = @() }
$stage = 'inputs'; $ready = $false; $attemptedInstall = $false; $installRoot = $null; $contract = $null; $workRoot = $null; $repoRoot = $null
function Add-SmokeStep { param([string]$Name, [bool]$Ok, $Detail); $script:result.steps += @{ stage = $Name; ok = $Ok; detail = $Detail } }
function Assert-Smoke { param([bool]$Value, [string]$Reason = 'Verificacao do smoke falhou'); if (-not $Value) { throw $Reason } }
function Get-SmokeTree {
    param([string]$Root)
    Assert-Smoke (-not ((Get-Item -LiteralPath $Root).Attributes -band [IO.FileAttributes]::ReparsePoint))
    $tree = @{}; $prefix = [IO.Path]::GetFullPath($Root).TrimEnd('\') + '\'
    $pending = New-Object 'System.Collections.Generic.Queue[string]'; $pending.Enqueue($Root)
    while ($pending.Count -gt 0) {
        foreach ($item in @(Get-ChildItem -LiteralPath $pending.Dequeue() -Force)) {
            Assert-Smoke (-not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint))
            Assert-Smoke ($item.FullName.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase))
            if ($item.PSIsContainer) { $pending.Enqueue($item.FullName) } else {
                $tree[$item.FullName.Substring($prefix.Length).Replace('\', '/')] = Get-SmokeHash $item.FullName
            }
        }
    }
    return $tree
}
function Get-SmokeRegistration {
    param([Microsoft.Win32.RegistryHive]$Hive, [string]$SubKey)
    $entries = @()
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32)) {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($Hive, $view)
        try {
            $key = $base.OpenSubKey($SubKey)
            if ($null -ne $key) {
                try { $entries += @{ location = ([string]$key.GetValue('InstallLocation', '')).Trim('"'); version = [string]$key.GetValue('DisplayVersion', '') } } finally { $key.Dispose() }
            }
        } finally { $base.Dispose() }
    }
    return $entries
}
function Invoke-SmokeProcess {
    param([string]$File, [string]$Arguments, [int]$TimeoutSeconds = 600)
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -WorkingDirectory $script:workRoot -WindowStyle Hidden -PassThru
    try {
        $null = $process.Handle # Manter handle aberto para ler ExitCode tambem no Windows PowerShell5.1.
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) { $process.Kill(); throw 'Timeout do processo proprio' }
        $process.Refresh()
        Assert-Smoke ($process.ExitCode -eq 0) "Processo NSIS terminou com exit code $($process.ExitCode)"
    } finally { $process.Dispose() }
}
function Confirm-SmokeRegistration {
    param([version]$ExpectedVersion, [string]$ExpectedDisplay)
    $entries = @(Get-SmokeRegistration CurrentUser $script:contract.uninstallKey)
    Assert-Smoke ($entries.Count -gt 0)
    foreach ($entry in $entries) {
        Assert-Smoke ([string]::Equals($entry.location, $script:installRoot, [StringComparison]::OrdinalIgnoreCase))
        Assert-Smoke ((Convert-SmokeVersion $entry.version) -eq $ExpectedVersion)
        if ($ExpectedDisplay) { Assert-Smoke ($entry.version -ceq $ExpectedDisplay) }
    }
    Assert-Smoke ((Convert-SmokeVersion (Get-Item -LiteralPath (Join-Path $script:installRoot $script:contract.main)).VersionInfo.ProductVersion) -eq $ExpectedVersion)
    Assert-Smoke (@(Get-SmokeRegistration LocalMachine $script:contract.uninstallKey).Count -eq 0)
}
function Remove-SmokeInstallation {
    # Nunca usar a UninstallString do registro: somente o arquivo no destino reservado.
    Assert-Smoke (-not ((Get-Item -LiteralPath $script:installRoot).Attributes -band [IO.FileAttributes]::ReparsePoint))
    $uninstaller = Join-Path $script:installRoot 'uninstall.exe'
    Assert-Smoke (Test-Path -LiteralPath $uninstaller -PathType Leaf)
    foreach ($entry in @(Get-SmokeRegistration CurrentUser $script:contract.uninstallKey)) {
        Assert-Smoke ([string]::Equals($entry.location, $script:installRoot, [StringComparison]::OrdinalIgnoreCase))
    }
    Invoke-SmokeProcess $uninstaller (Get-SmokeArguments $script:installRoot -Uninstall) 180
    # _?= aguarda a execucao no lugar; NSIS nao consegue apagar o proprio EXE aberto.
    $left = @{}
    if (Test-Path -LiteralPath $script:installRoot) { $left = Get-SmokeTree $script:installRoot }
    $remaining = @($left.Keys | Where-Object { $_ -ne 'uninstall.exe' })
    Assert-Smoke ($remaining.Count -eq 0) "Payload remanescente apos desinstalar: $($remaining -join ', ')"
    if (Test-Path -LiteralPath $uninstaller) { Remove-Item -LiteralPath $uninstaller -Force }
    foreach ($hive in @('CurrentUser', 'LocalMachine')) {
        foreach ($key in @($script:contract.uninstallKey, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\madscreen viewer')) {
            Assert-Smoke (@(Get-SmokeRegistration $hive $key).Count -eq 0) "Chave de desinstalacao remanescente: $hive/$key"
        }
    }
    # Apenas diretorios ja vazios; nunca limpar payload residual com -Recurse.
    if (Test-Path -LiteralPath $script:installRoot) {
        foreach ($dir in @(Get-ChildItem -LiteralPath $script:installRoot -Directory -Recurse | Sort-Object { $_.FullName.Length } -Descending)) {
            Remove-Item -LiteralPath $dir.FullName -ErrorAction Stop
        }
        Remove-Item -LiteralPath $script:installRoot -ErrorAction Stop
    }
    Assert-Smoke (-not (Test-Path -LiteralPath $script:installRoot))
    Add-SmokeStep 'uninstall' $true @{ payloadRemoved = $true; uninstallRegistryRemoved = $true; inPlaceUninstallerRemoved = $true;
        installLocationPreferencePreserved = (@(Get-SmokeRegistration CurrentUser $script:contract.locationKey).Count -gt 0) }
}

try {
    Assert-Smoke ($env:OS -eq 'Windows_NT')
    $workRoot = [IO.Path]::GetFullPath($WorkDir)
    Assert-Smoke (-not (Test-Path -LiteralPath $workRoot))
    New-Item -ItemType Directory -Path $workRoot | Out-Null; $ready = $true
    $installRoot = Join-Path $workRoot 'installed'
    $repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
    $appRoot = Join-Path $repoRoot 'apps/madscreen-app/src-tauri'
    $config = Get-Content -LiteralPath (Join-Path $appRoot 'tauri.conf.json') -Raw | ConvertFrom-Json
    $windowsConfig = Get-Content -LiteralPath (Join-Path $appRoot 'tauri.windows.conf.json') -Raw | ConvertFrom-Json
    $contract = Get-SmokeContract $config $windowsConfig (Get-Content -LiteralPath (Join-Path $appRoot 'windows/installer.nsi') -Raw) (Get-Content -LiteralPath (Join-Path $appRoot 'windows/hooks.nsh') -Raw)
    $Installer = (Get-Item -LiteralPath $Installer -ErrorAction Stop).FullName
    Assert-Smoke ($Installer.EndsWith('.exe', [StringComparison]::OrdinalIgnoreCase))
    $newVersion = Convert-SmokeVersion (Get-Item -LiteralPath $Installer).VersionInfo.ProductVersion
    $displayVersion = '{0}.{1}.{2}' -f $newVersion.Major, $newVersion.Minor, $newVersion.Build
    $result.installerSha256 = Get-SmokeHash $Installer
    Add-SmokeStep 'inputs' $true @{ installMode = $contract.mode; installDirectory = '[work]/installed'; uninstallHive = 'HKCU'; uninstallKey = $contract.uninstallKey; version = $displayVersion }

    $stage = 'clean-machine'
    foreach ($hive in @('CurrentUser', 'LocalMachine')) {
        foreach ($key in @($contract.uninstallKey, $contract.locationKey, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\madscreen viewer')) {
            Assert-Smoke (@(Get-SmokeRegistration $hive $key).Count -eq 0)
        }
    }
    Assert-Smoke (-not (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA $contract.product)))
    Assert-Smoke (@(Get-Process -Name 'madscreen-app', 'madscreen-encode-test' -ErrorAction SilentlyContinue).Count -eq 0)
    Add-SmokeStep $stage $true 'Nenhuma instalacao/registro/processo preexistente; destino reservado novo'

    # Referencia autoritativa: os bytes que O PROPRIO instalador novo instala, sem build local.
    # Mesmo destino elimina diferencas legitimas dependentes de caminho. Nao abrir o app.
    $stage = 'install-new-clean'; $attemptedInstall = $true
    Invoke-SmokeProcess $Installer (Get-SmokeArguments $installRoot)
    Confirm-SmokeRegistration $newVersion $displayVersion
    $expected = Get-SmokeTree $installRoot
    Assert-Smoke ($expected.ContainsKey('uninstall.exe')) 'Instalacao limpa sem desinstalador'
    $expected.Remove('uninstall.exe')
    Add-SmokeStep $stage $true @{ files = $expected.Count; payloadSha256 = $expected; source = 'new-installer-clean-install' }
    $buildExe = Join-Path $appRoot "target/release/$($contract.main)"
    if (Test-Path -LiteralPath $buildExe) {
        $buildHash = Get-SmokeHash $buildExe
        Add-SmokeStep 'build-comparison-diagnostic' $true @{ file = $contract.main; buildSha256 = $buildHash;
            packagedSha256 = $expected[$contract.main]; equal = ($buildHash -eq $expected[$contract.main]); authoritative = $false }
    }
    $stage = 'uninstall-clean-reference'; Remove-SmokeInstallation; $attemptedInstall = $false

    $stage = 'runtime-reference'
    $manifest = Get-Content -LiteralPath (Join-Path $repoRoot 'tools/encode/runtime-gpl.json') -Raw | ConvertFrom-Json
    $archive = Join-Path (Join-Path $repoRoot 'target/encoder-runtime') $manifest.archiveName
    if (-not (Test-Path -LiteralPath $archive)) {
        $archive = Join-Path $workRoot $manifest.archiveName
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        $client = New-Object Net.WebClient
        try { $client.DownloadFile($manifest.url, $archive) } finally { $client.Dispose() }
    }
    Assert-Smoke ((Get-SmokeHash $archive) -eq $manifest.sha256) 'SHA256 do ZIP de runtime divergente'
    $reference = Join-Path $workRoot 'runtime-reference'
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::ExtractToDirectory($archive, $reference)
    $bundleTree = @{}
    foreach ($name in $expected.Keys) {
        if ($name.StartsWith('encoder-runtime/')) { $bundleTree[$name.Substring('encoder-runtime/'.Length)] = $expected[$name] }
    }
    foreach ($dll in $manifest.bundleDlls) {
        Assert-Smoke ($bundleTree.ContainsKey($dll))
        Assert-Smoke ($bundleTree[$dll] -eq (Get-SmokeHash (Join-Path (Join-Path $reference 'bin') $dll)))
    }
    Assert-Smoke (@($bundleTree.Keys | Where-Object { $_ -like '*.dll' -and @($manifest.bundleDlls) -notcontains $_ }).Count -eq 0)
    foreach ($pair in @(@('LICENSE', 'LICENSE'), @('README.txt', 'README.txt'), @('SOURCE-OFFER.md', 'notices/SOURCE-OFFER.md'), @('x264-GPL-2.0.txt', 'notices/x264-GPL-2.0.txt'))) {
        Assert-Smoke ($bundleTree.ContainsKey($pair[0]))
        Assert-Smoke ($bundleTree[$pair[0]] -eq (Get-SmokeHash (Join-Path $reference $pair[1])))
    }
    Assert-Smoke ($bundleTree.ContainsKey('NOTICE.md'))
    Assert-Smoke ($bundleTree['NOTICE.md'] -eq (Get-SmokeHash (Join-Path $repoRoot 'tools/encode/NOTICE.md')))
    Add-SmokeStep $stage $true @{ archiveSha256 = $manifest.sha256; dlls = @($manifest.bundleDlls); installedLayout = 'encoder-runtime/ (DLLs e avisos planos)' }

    $old = @{}
    if ($PreviousInstaller) {
        $stage = 'install-previous'
        $PreviousInstaller = (Get-Item -LiteralPath $PreviousInstaller).FullName
        Assert-Smoke ($PreviousInstaller.EndsWith('.exe', [StringComparison]::OrdinalIgnoreCase))
        $previousVersion = Convert-SmokeVersion (Get-Item -LiteralPath $PreviousInstaller).VersionInfo.ProductVersion
        Assert-Smoke ($previousVersion -le $newVersion)
        $result.previousInstallerSha256 = Get-SmokeHash $PreviousInstaller
        $result.upgrade = $(if ($previousVersion -eq $newVersion) { 'reinstall-same-version' } else { 'upgrade' })
        $attemptedInstall = $true
        Invoke-SmokeProcess $PreviousInstaller (Get-SmokeArguments $installRoot)
        Confirm-SmokeRegistration $previousVersion
        $old = Get-SmokeTree $installRoot
        Add-SmokeStep $stage $true @{ version = $previousVersion.ToString(); files = $old.Count }
    } else { Add-SmokeStep 'install-previous' $true 'sem upgrade: release anterior nao fornecida/acessivel' }

    $stage = 'install-new'; $attemptedInstall = $true
    Invoke-SmokeProcess $Installer (Get-SmokeArguments $installRoot)
    Confirm-SmokeRegistration $newVersion $displayVersion
    $actual = Get-SmokeTree $installRoot
    $comparison = Compare-SmokePayload $expected $actual
    # Nao apagar sobras para fazer o teste passar: flags extras revelam upgrade incompleto.
    Add-SmokeStep 'installed-payload' $comparison.ok $comparison
    Add-SmokeStep 'payload-evidence' $comparison.ok (Get-SmokePayloadEvidence $expected $actual $old)
    Assert-Smoke $comparison.ok "Payload divergiu do proprio instalador novo: $($comparison | ConvertTo-Json -Compress)"
    Assert-Smoke ($actual.ContainsKey('uninstall.exe'))
    $changed = @($expected.Keys | Where-Object { $old.ContainsKey($_) -and $old[$_] -ne $actual[$_] })
    Add-SmokeStep $stage $true @{ version = $displayVersion; expectedFiles = $expected.Count; filesMatchNewInstaller = $true; previousFiles = $old.Count; replacedFiles = $changed; orphanFiles = @() }
    Add-SmokeStep 'installed-runtime' $true @{ dllsMatchPinnedArchive = $true; noticesMatchReference = $true; runtimeDirectory = 'encoder-runtime'; noticesLayout = 'flat' }

    $stage = 'installed-sidecar'
    $sidecar = Join-Path $installRoot 'madscreen-encode-test.exe'
    $smokeDir = Join-Path $workRoot 'isolated-cwd'; New-Item -ItemType Directory -Path $smokeDir | Out-Null
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $sidecar; $info.WorkingDirectory = $smokeDir
    $info.Arguments = '--encoder x264 --source synthetic --synthetic static --size 320x180 --fps 30 --bitrate 1 --warmup 0 --duration 3 --output stream.h264 --report report.json'
    $info.UseShellExecute = $false; $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
    foreach ($name in @($info.EnvironmentVariables.Keys)) { if ($name -match '^(MADSCREEN_|AV20_)') { $info.EnvironmentVariables.Remove($name) } }
    $info.EnvironmentVariables['PATH'] = "$env:SystemRoot\System32;$env:SystemRoot;$env:SystemRoot\System32\Wbem"
    $info.EnvironmentVariables['MADSCREEN_ENCODER_RUNTIME'] = Join-Path $installRoot 'encoder-runtime'
    $process = New-Object Diagnostics.Process; $process.StartInfo = $info
    try {
        Assert-Smoke ($process.Start()); $stdout = $process.StandardOutput.ReadToEndAsync(); $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(60000)) { $process.Kill(); throw 'Timeout do sidecar instalado' }
        $process.WaitForExit(); $null = $stdout.Result; $errorText = $stderr.Result
        $diagnostic = $null
        if ($process.ExitCode -ne 0) { $diagnostic = Get-SmokeSidecarDiagnostic $errorText }
        Add-SmokeStep 'sidecar-exit' ($process.ExitCode -eq 0) @{ exitCode = $process.ExitCode; backendRequested = 'x264'; hardwareEncoderRequested = $false; diagnostic = $diagnostic }
        Assert-Smoke ($process.ExitCode -eq 0)
        $encoded = Get-Content -LiteralPath (Join-Path $smokeDir 'report.json') -Raw | ConvertFrom-Json
        Assert-Smoke ($encoded.encoded -gt 0 -and $encoded.encoder -match 'x264')
        Assert-Smoke ((Get-Item -LiteralPath (Join-Path $smokeDir 'stream.h264')).Length -gt 0)
        Add-SmokeStep $stage $true @{ frames = $encoded.encoded; encoder = $encoded.encoder; secondsRequested = 3; pathIsolated = $true; installedSidecarSha256 = $actual['madscreen-encode-test.exe'] }
    } finally { $process.Dispose() }
    $stage = 'uninstall'; Remove-SmokeInstallation; $attemptedInstall = $false
    $result.ok = $true
} catch {
    Add-SmokeStep $stage $false (Get-SmokeErrorDetail $_ 'stage-failed' @($workRoot, $repoRoot, $env:USERPROFILE, $Installer, $PreviousInstaller))
} finally {
    if ($attemptedInstall -and $contract -and $installRoot) {
        if (Test-Path -LiteralPath (Join-Path $installRoot 'uninstall.exe')) {
            try { Remove-SmokeInstallation } catch { Add-SmokeStep 'cleanup' $false (Get-SmokeErrorDetail $_ 'uninstall-failed' @($workRoot, $repoRoot, $env:USERPROFILE, $Installer, $PreviousInstaller)); $result.ok = $false }
        } else {
            Add-SmokeStep 'cleanup' $false @{ code = 'owned-uninstaller-missing'; unsafeFallbackRefused = $true }; $result.ok = $false
        }
    }
    if ($ready) { $result | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $workRoot 'result.json') -Encoding UTF8 }
    $result | ConvertTo-Json -Depth 12 -Compress | Write-Output
}
if (-not $result.ok) { exit 1 }
