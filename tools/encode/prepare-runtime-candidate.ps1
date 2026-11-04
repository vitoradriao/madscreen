<# Candidato isolado e fixado por hash; nunca substitui runtime instalado/bundle. #>
[CmdletBinding()]
param([string]$Destination = "$PSScriptRoot/../../target/runtime-lgpl-candidate")
$ErrorActionPreference = 'Stop'
$manifest = Get-Content -LiteralPath "$PSScriptRoot/runtime-candidate.json" -Raw | ConvertFrom-Json
$destinationPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Destination)
if (Test-Path -LiteralPath $destinationPath) { throw 'Destino ja existe; preserve a bancada anterior e use uma pasta nova.' }
New-Item -ItemType Directory -Path $destinationPath | Out-Null
foreach ($artifact in @(
    @{ Name = 'runtime.zip'; Url = $manifest.archiveUrl; Hash = $manifest.archiveSha256 },
    @{ Name = 'recipe.zip'; Url = $manifest.recipeUrl; Hash = $manifest.recipeSha256 }
)) {
    $artifactPath = Join-Path $destinationPath $artifact.Name
    Invoke-WebRequest -Uri $artifact.Url -OutFile $artifactPath
    if ((Get-FileHash -LiteralPath $artifactPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $artifact.Hash) { throw 'Hash do candidato divergiu; nenhuma promocao permitida.' }
    Expand-Archive -LiteralPath $artifactPath -DestinationPath (Join-Path $destinationPath $artifact.Name.Replace('.zip',''))
}
Copy-Item -LiteralPath "$PSScriptRoot/runtime-candidate.json" -Destination (Join-Path $destinationPath 'manifest.json')
$candidateFiles = Get-ChildItem -LiteralPath (Join-Path $destinationPath 'runtime') -Recurse -File
$inventory = foreach ($candidateFile in $candidateFiles) {
    [ordered]@{ path = $candidateFile.FullName.Substring($destinationPath.Length + 1); bytes = $candidateFile.Length; sha256 = (Get-FileHash -LiteralPath $candidateFile.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
}
[ordered]@{ schema = 1; distributionApproved = $false; files = @($inventory); blockers = $manifest.promotionBlockedBy } |
    ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $destinationPath 'inventory.json') -Encoding UTF8
Write-Output "Candidato de pesquisa preparado, sem promocao: $destinationPath"
