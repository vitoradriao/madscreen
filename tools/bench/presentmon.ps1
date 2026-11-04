[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ProcessName,
    [Parameter(Mandatory = $true)][string]$OutputCsv,
    [string]$PresentMonPath = 'PresentMon.exe',
    [ValidateRange(0, 86400)][int]$WarmupSec = 60,
    [ValidateRange(1, 86400)][int]$DurationSec = 180
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$command = Get-Command $PresentMonPath -ErrorAction Stop
$parent = Split-Path -Parent $OutputCsv
if ($parent) { [void](New-Item -ItemType Directory -Path $parent -Force) }
if (Test-Path -LiteralPath $OutputCsv) { throw "O CSV de saída já existe: $OutputCsv" }
# GPU tracking padrão; não usar --no_track_gpu nem --track_gpu_video.
& $command.Source --process_name $ProcessName --delay $WarmupSec --timed $DurationSec --terminate_after_timed --no_console_stats --stop_existing_session --output_file $OutputCsv --no_track_input --v2_metrics | Out-Host
if ($LASTEXITCODE -ne 0) { throw "PresentMon retornou código $LASTEXITCODE." }
if (-not (Test-Path -LiteralPath $OutputCsv) -or (Get-Item -LiteralPath $OutputCsv).Length -eq 0) {
    throw 'PresentMon não gerou um CSV não vazio.'
}
$reader = [System.IO.File]::OpenText($OutputCsv)
try {
    $header = $reader.ReadLine()
    $data = $reader.ReadLine()
    if ([string]::IsNullOrWhiteSpace($header) -or [string]::IsNullOrWhiteSpace($data)) {
        throw 'CSV do PresentMon sem linhas de frames.'
    }
} finally { $reader.Dispose() }
