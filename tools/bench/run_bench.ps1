[CmdletBinding()]
param(
    [string]$Config = (Join-Path $PSScriptRoot 'examples/bench.example.json'),
    [ValidateRange(1, 100)][int]$Runs = 5,
    [ValidateRange(1, 86400)][int]$DurationSec = 180,
    [ValidateRange(0, 86400)][int]$WarmupSec = 60,
    [ValidateRange(0, 2147483645)][int]$Seed = 42,
    [string]$PresentMonPath = 'PresentMon.exe',
    [string]$OutDir = ('bench-results/' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff')),
    [switch]$DryRun
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'bench-common.ps1')
$configData = Get-Content -LiteralPath $Config -Encoding UTF8 -Raw | ConvertFrom-Json
foreach ($field in @('games', 'scenarios', 'conditions')) {
    $items = @(Get-BenchProperty $configData $field @())
    if ($items.Count -eq 0) { throw "Configuração sem $field." }
    $ids = @{}
    foreach ($item in $items) {
        $id = [string](Get-BenchProperty $item 'id' '')
        if ([string]::IsNullOrWhiteSpace($id) -or $ids.ContainsKey($id)) { throw "ID ausente ou duplicado em $field." }
        $ids[$id] = $true
    }
}
foreach ($game in $configData.games) {
    if ([string]::IsNullOrWhiteSpace([string](Get-BenchProperty $game 'exe' ''))) { throw 'Jogo sem exe.' }
}
$baseline = [string](Get-BenchProperty $configData 'baseline_condition' 'baseline')
$baselineEntries = @($configData.conditions | Where-Object { $_.id -eq $baseline })
if ($baselineEntries.Count -ne 1 -or (Get-BenchProperty $baselineEntries[0] 'skip' $false)) { throw 'Configuração exige um baseline habilitado.' }
if (Test-Path -LiteralPath $OutDir) {
    if (@(Get-ChildItem -LiteralPath $OutDir -Force).Count -gt 0) { throw "Diretório de saída deve estar vazio: $OutDir" }
}
[void](New-Item -ItemType Directory -Path $OutDir -Force)
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path
$manifestPath = Join-Path $OutDir 'manifest.json'
Copy-Item -LiteralPath $Config -Destination (Join-Path $OutDir 'config.json')
$manifest = [ordered]@{
    schema_version = 1; created_at = [DateTime]::UtcNow.ToString('o'); synthetic = [bool]$DryRun
    seed = $Seed; runs_per_condition = $Runs; duration_sec = $DurationSec; warmup_sec = $WarmupSec
    baseline_condition = $baseline; environment = (Get-BenchEnvironment $PresentMonPath -DryRun:$DryRun); runs = @()
}
$shuffleState = @{ Value = [long]$Seed + 1 }
$order = 0
$hadFailure = $false
Write-BenchJson $manifest $manifestPath
foreach ($game in $configData.games) {
    try {
        if (-not $DryRun) {
            Invoke-BenchCommand (Get-BenchProperty $game 'start')
            if (Get-BenchProperty $game 'manual' $false) { [void](Read-Host ("Prepare o jogo {0}, replay/cena e pressione Enter" -f $game.id)) }
        }
        foreach ($scenario in $configData.scenarios) {
            try {
                if (-not $DryRun) {
                    Invoke-BenchCommand (Get-BenchProperty $scenario 'start')
                    if (Get-BenchProperty $scenario 'manual' $true) { [void](Read-Host ("Prepare {0}/{1}: {2}. Pressione Enter" -f $game.id, $scenario.id, (Get-BenchProperty $scenario 'description' ''))) }
                }
                for ($round = 1; $round -le $Runs; $round++) {
                    $conditions = @(Get-ShuffledConditions @($configData.conditions) $shuffleState)
                    foreach ($condition in $conditions) {
                        $order++
                        $id = 'run-{0:D5}' -f $order
                        $entry = [ordered]@{
                            id = $id; game = [string]$game.id; scenario = [string]$scenario.id; condition = [string]$condition.id
                            round = $round; order = $order; application = [string]$game.exe; status = 'failed'; csv = $null
                            duration_sec = $DurationSec; warmup_sec = $WarmupSec; notes = 'Corrida interrompida antes de concluir.'
                            gpu_before = $null; gpu_after = $null; gpu_samples_csv = $null
                        }
                        $manifest.runs += ,$entry
                        Write-BenchJson $manifest $manifestPath
                        Write-Host ("{0}: {1}/{2}/{3}, rodada {4}" -f $id, $game.id, $scenario.id, $condition.id, $round)
                        $skip = (-not $DryRun) -and [bool](Get-BenchProperty $condition 'skip' $false)
                        $job = $null
                        $conditionPrepared = $false
                        $cleanupFailed = $false
                        try {
                            if (-not $DryRun -and -not $skip) {
                                $conditionPrepared = $true
                                Invoke-BenchCommand (Get-BenchProperty $condition 'start')
                                Invoke-BenchCommand (Get-BenchProperty $condition 'check')
                                if (Get-BenchProperty $condition 'manual' $false) {
                                    $answer = Read-Host ("Prepare {0}, deixe SOMENTE esta condição ativa e reinicie a cena/replay. Enter para capturar; s para pular" -f $condition.id)
                                    if ($answer -eq 's') { $skip = $true }
                                }
                            }
                            if ($skip) {
                                $entry.status = 'skipped'; $entry.notes = [string](Get-BenchProperty $condition 'notes' 'Condição pulada pelo operador/configuração.')
                            } else {
                                $entry.csv = $id + '.csv'
                                $csvPath = Join-Path $OutDir $entry.csv
                                if ($DryRun) {
                                    $fps = 144.0
                                    if ($scenario.id -eq 'S-B') { $fps = 200.0 }
                                    if ($condition.id -ne $baseline) { $fps *= 0.97 }
                                    $fps *= 1.0 + (($round - 3) * 0.0005)
                                    $captureSeed = [int]((([long]$Seed + $order * 7919) % 2147483645))
                                    Write-SyntheticCapture $csvPath $game.exe $DurationSec $fps $captureSeed
                                } else {
                                    $entry.gpu_before = Get-GpuSnapshot
                                    $sampleName = $id + '-gpu.csv'
                                    $job = Start-GpuSampling (Join-Path $OutDir $sampleName)
                                    if ($null -ne $job) { $entry.gpu_samples_csv = $sampleName }
                                    & (Join-Path $PSScriptRoot 'presentmon.ps1') -ProcessName $game.exe -OutputCsv $csvPath -PresentMonPath $PresentMonPath -WarmupSec $WarmupSec -DurationSec $DurationSec
                                    Invoke-BenchCommand (Get-BenchProperty $condition 'check')
                                }
                                $entry.status = 'complete'; $entry.notes = $null
                            }
                        } catch {
                            $entry.status = 'failed'; $entry.notes = $_.Exception.Message; $hadFailure = $true
                            Write-Warning ("Falha em {0}: {1}" -f $id, $entry.notes)
                        } finally {
                            if ($null -ne $job) {
                                Stop-Job $job
                                Receive-Job $job -ErrorAction Continue | Out-Host
                                Remove-Job $job
                                if (-not (Test-Path -LiteralPath (Join-Path $OutDir $entry.gpu_samples_csv))) { $entry.gpu_samples_csv = $null }
                            }
                            if (-not $DryRun -and -not $skip) { $entry.gpu_after = Get-GpuSnapshot }
                            if ($conditionPrepared) {
                                try {
                                    Invoke-BenchCommand (Get-BenchProperty $condition 'stop')
                                    if (Get-BenchProperty $condition 'manual' $false) {
                                        [void](Read-Host ("Encerre {0} e confirme que nenhuma transmissão ficou ativa. Pressione Enter" -f $condition.id))
                                    }
                                }
                                catch { $entry.status = 'failed'; $entry.notes = "Falha ao encerrar condição: $($_.Exception.Message)"; $hadFailure = $true; $cleanupFailed = $true }
                            }
                            Write-BenchJson $manifest $manifestPath
                        }
                        if ($cleanupFailed) { throw 'Coleta interrompida: não foi possível encerrar a condição anterior. Analise o manifest salvo.' }
                    }
                }
            } finally { if (-not $DryRun) { Invoke-BenchCommand (Get-BenchProperty $scenario 'stop') } }
        }
    } finally { if (-not $DryRun) { Invoke-BenchCommand (Get-BenchProperty $game 'stop') } }
}
$analyzer = Get-Command 'madscreen-bench' -ErrorAction SilentlyContinue
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$localAnalyzer = Join-Path $repoRoot 'target/debug/madscreen-bench'
if ($env:OS -eq 'Windows_NT') { $localAnalyzer += '.exe' }
if ($null -ne $analyzer) { $localAnalyzer = $analyzer.Source }
if (Test-Path -LiteralPath $localAnalyzer) {
    # Windows PowerShell 5.1 trata stderr de programas nativos como erro quando ErrorActionPreference é Stop.
    $ErrorActionPreference = 'Continue'
    & $localAnalyzer analyze $OutDir --strict
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) { throw 'A análise falhou. Consulte o relatório no diretório de saída.' }
} else { Write-Warning 'madscreen-bench não encontrado. Execute cargo run -p madscreen-bench -- analyze <diretório> --strict.' }
if ($hadFailure) { throw 'Uma ou mais corridas falharam. Consulte manifest.json.' }
Write-Host "Coleta concluída: $OutDir"
