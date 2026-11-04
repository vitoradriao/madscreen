# Funções compartilhadas compatíveis com Windows PowerShell 5.1 e PowerShell 7.
Set-StrictMode -Version Latest

function Write-BenchJson {
    param([Parameter(Mandatory = $true)]$Value, [Parameter(Mandatory = $true)][string]$Path)
    $json = ConvertTo-Json -InputObject $Value -Depth 30
    $encoding = New-Object System.Text.UTF8Encoding($false)
    # Renomear um arquivo completo evita deixar JSON truncado ao interromper uma corrida.
    [System.IO.File]::WriteAllText(($Path + '.tmp'), $json, $encoding)
    Move-Item -LiteralPath ($Path + '.tmp') -Destination $Path -Force
}

function Get-BenchProperty {
    param($Object, [string]$Name, $Default = $null)
    if ($null -ne $Object -and $null -ne $Object.PSObject.Properties[$Name]) { return $Object.$Name }
    return $Default
}

# Park-Miller: sequência definida aqui para reproduzir a ordem também entre runtimes .NET.
function Get-BenchRandom {
    param([hashtable]$State)
    $State.Value = ([long]$State.Value * 48271) % 2147483647
    return ([double]$State.Value / 2147483647.0)
}

function Get-ShuffledConditions {
    param([object[]]$Items, [hashtable]$State)
    $copy = @($Items.Clone())
    for ($i = $copy.Count - 1; $i -gt 0; $i--) {
        $j = [int][math]::Floor((Get-BenchRandom $State) * ($i + 1))
        $saved = $copy[$i]; $copy[$i] = $copy[$j]; $copy[$j] = $saved
    }
    return $copy
}

function Get-GpuSnapshot {
    $command = Get-Command 'nvidia-smi' -ErrorAction SilentlyContinue
    if ($null -eq $command) { return $null }
    try {
        $lines = @(& $command.Source '--query-gpu=index,name,driver_version,clocks.sm,clocks.mem,temperature.gpu,power.draw,pstate,utilization.gpu,utilization.encoder' '--format=csv,noheader,nounits' 2>$null)
        if ($LASTEXITCODE -ne 0 -or $lines.Count -eq 0) { return $null }
        $headers = @('gpu_index', 'gpu', 'driver', 'clocks_sm_mhz', 'clocks_mem_mhz', 'temperature_c', 'power_w', 'pstate', 'utilization_gpu_percent', 'utilization_encoder_percent')
        $result = @($lines | ConvertFrom-Csv -Header $headers | ForEach-Object {
            $row = [ordered]@{ timestamp_utc = [DateTime]::UtcNow.ToString('o') }
            foreach ($name in $headers) {
                $value = $_.$name.Trim()
                if ($value -match '^(N/A|\[N/A\]|Not Supported|\[Not Supported\])$') { $value = $null }
                $row[$name] = $value
            }
            [pscustomobject]$row
        })
        return ,$result
    } catch { return $null }
}

function Get-BenchEnvironment {
    param([string]$PresentMonPath, [switch]$DryRun)
    $gpu = $null
    if (-not $DryRun) { $gpu = Get-GpuSnapshot }
    $gpuName = $null; $driver = $null; $plan = $null; $windows = $null; $version = $null
    if ($null -ne $gpu -and @($gpu).Count -gt 0) {
        $gpuName = (@($gpu | ForEach-Object { $_.gpu }) -join '; ')
        $driver = (@($gpu | ForEach-Object { $_.driver }) -join '; ')
    }
    if ($env:OS -eq 'Windows_NT') {
        $windows = [Environment]::OSVersion.VersionString
        try {
            $os = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
            $windows = $os.Caption + ' ' + $os.Version + ' (build ' + $os.BuildNumber + ')'
        } catch { }
        if ($null -ne (Get-Command powercfg -ErrorAction SilentlyContinue)) {
            try { $plan = ((& powercfg /getactivescheme 2>$null) -join ' ').Trim() } catch { }
        }
    }
    if ($DryRun) { $version = 'sintético (formato PresentMon 2.x)' }
    else {
        $pm = Get-Command $PresentMonPath -ErrorAction SilentlyContinue
        if ($null -ne $pm) {
            try { $version = (Get-Item -LiteralPath $pm.Source).VersionInfo.ProductVersion } catch { }
        }
    }
    return [ordered]@{ gpu = $gpuName; driver = $driver; power_plan = $plan; windows_version = $windows; presentmon_version = $version }
}

function Start-GpuSampling {
    param([string]$Path, [int]$IntervalSec = 2)
    if ($null -eq (Get-Command nvidia-smi -ErrorAction SilentlyContinue)) { return $null }
    $commonPath = Join-Path $PSScriptRoot 'bench-common.ps1'
    return Start-Job -ArgumentList $commonPath, $Path, $IntervalSec -ScriptBlock {
        param($CommonPath, $CsvPath, $Seconds)
        . $CommonPath
        while ($true) {
            $snapshot = Get-GpuSnapshot
            if ($null -ne $snapshot) {
                $snapshot | Export-Csv -LiteralPath $CsvPath -NoTypeInformation -Append -Encoding UTF8
            }
            Start-Sleep -Seconds $Seconds
        }
    }
}

function Invoke-BenchCommand {
    param($Command)
    if (-not [string]::IsNullOrWhiteSpace([string]$Command)) {
        $global:LASTEXITCODE = 0
        & ([scriptblock]::Create([string]$Command)) | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Comando da configuração retornou código $LASTEXITCODE." }
    }
}

function Write-SyntheticCapture {
    param([string]$Path, [string]$Application, [int]$DurationSec, [double]$Fps, [int]$Seed)
    $state = @{ Value = [long]$Seed + 1 }
    $culture = [Globalization.CultureInfo]::InvariantCulture
    $writer = New-Object System.IO.StreamWriter($Path, $false, (New-Object System.Text.UTF8Encoding($false)))
    try {
        $writer.WriteLine('Application,ProcessID,SwapChainAddress,CPUStartTime,FrameTime,GPUBusy,GPUWait,CPUBusy,DisplayLatency,DisplayedTime,PresentMode,AllowsTearing')
        $time = 0.0
        $app = '"' + $Application.Replace('"', '""') + '"'
        while ($time -lt ($DurationSec * 1000.0)) {
            $ms = (1000.0 / $Fps) * (1.0 + ((Get-BenchRandom $state) - 0.5) * 0.01)
            $writer.WriteLine(('{0},1234,0x1,{1},{2},{3},0.1,1.0,8.0,{2},Composed: Flip,1' -f $app, $time.ToString('F6', $culture), $ms.ToString('F6', $culture), ($ms * 0.7).ToString('F6', $culture)))
            $time += $ms
        }
    } finally { $writer.Dispose() }
}
