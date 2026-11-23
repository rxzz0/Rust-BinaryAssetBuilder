[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$SdkRoot,
    [Parameter(Mandatory)][string]$RustCompiler,
    [Parameter(Mandatory)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$sdk = (Resolve-Path -LiteralPath $SdkRoot).Path
$ea = (Resolve-Path -LiteralPath (Join-Path $sdk 'tools/BinaryAssetBuilder.exe')).Path
$rust = (Resolve-Path -LiteralPath $RustCompiler).Path
$fixtures = Join-Path $PSScriptRoot '../tests/fixtures/ea-conformance'
if (Test-Path -LiteralPath $OutputDirectory) {
    throw 'OutputDirectory must be a new directory'
}
$work = (New-Item -ItemType Directory -Path $OutputDirectory).FullName
$schemas = (New-Item -ItemType Directory -Path (Join-Path $work 'Schemas')).FullName
Copy-Item -LiteralPath (Join-Path $sdk 'Schemas/xsd') -Destination $schemas -Recurse

function Get-ReferenceWarnings([string]$Log) {
    # Strip compiler-specific timestamps and file locations while retaining the two asset names.
    $pattern = "(?im)Warning:\s*Unknown asset '([^']+)' referenced from '([^']+)'"
    [regex]::Matches([string](Get-Content -LiteralPath $Log -Raw), $pattern) |
        ForEach-Object { '{0} referenced from {1}' -f $_.Groups[1].Value, $_.Groups[2].Value } |
        Sort-Object
}

$passed = 0
$cases = @(Get-ChildItem -LiteralPath $fixtures -Directory | Sort-Object Name)
foreach ($case in $cases) {
    $source = Join-Path $work "Mods/$($case.Name)/Data"
    New-Item -ItemType Directory -Path $source | Out-Null
    Get-ChildItem -LiteralPath $case.FullName | Copy-Item -Destination $source -Recurse
    $inputXml = Join-Path $source 'mod.xml'
    $caseOutput = Join-Path $work "results/$($case.Name)"
    $eaOutput = (New-Item -ItemType Directory -Path (Join-Path $caseOutput 'ea')).FullName
    $rustOutput = Join-Path $caseOutput 'rust'
    $eaLog = Join-Path $caseOutput 'ea.log'
    $rustLog = Join-Path $caseOutput 'rust.log'
    $comparisonLog = Join-Path $caseOutput 'comparison.log'
    Write-Host "CASE $($case.Name)"

    # EA anchors its SDK search paths and stream paths at the configured data root.
    Push-Location -LiteralPath $work
    try {
        $modPath = "Mods\$($case.Name)"
        # Give both compilers the same logical StringHashes.xml path for manifest comparison.
        $hashDirectory = Join-Path $rustOutput "$modPath/Data"
        New-Item -ItemType Directory -Path $hashDirectory | Out-Null
        $hashXml = Join-Path $hashDirectory 'StringHashes.xml'
        $eaHashXml = Join-Path $eaOutput 'StringHashes.xml'
        & $ea $inputXml "/dr:$work" "/od:$eaOutput" "/iod:$eaOutput" "/scd:$hashDirectory" `
            "/data:.;.\$modPath\Data;.\Mods;.\SageXml" "/art:.;.\$modPath\Art;.\Mods;.\Art" `
            "/audio:.;.\$modPath\Audio;.\Mods;.\Audio" /bc:false /sc:false /ls:true /ss:true /vf:false /poe:false /osh:true *> $eaLog
        $eaExit = $LASTEXITCODE
    } finally {
        Pop-Location
    }
    if ($eaExit -ne 0) {
        Write-Host "  ORACLE FAILED (exit $eaExit). See $eaLog"
        continue
    }
    $eaManifests = @(Get-ChildItem -LiteralPath $eaOutput -Recurse -Filter '*.manifest' -File)
    if ('mod.manifest' -notin $eaManifests.Name) {
        Write-Host "  ORACLE FAILED (no mod.manifest). See $eaLog"
        continue
    }
    Move-Item -LiteralPath $hashXml -Destination $eaHashXml
    & $rust compile-static-sdk $inputXml --sdk-root $work --output $rustOutput --string-hashes *> $rustLog
    $rustExit = $LASTEXITCODE
    # The EA ModStudio script appends one blank line to the main stream's version file.
    $mainManifest = Join-Path $eaOutput "$modPath/Data/mod.manifest"
    [IO.File]::AppendAllText([IO.Path]::ChangeExtension($mainManifest, 'version'), "`r`n")
    $same = $rustExit -eq 0
    if (-not $same) {
        Write-Host "  DIFF build outcome (EA exit $eaExit, Rust exit $rustExit). See $rustLog"
    } else {
        if (!(Test-Path -LiteralPath $hashXml -PathType Leaf) -or
            (Get-FileHash -LiteralPath $eaHashXml).Hash -ne (Get-FileHash -LiteralPath $hashXml).Hash) {
            Write-Host '  DIFF StringHashes.xml'
            $same = $false
        }
        $rustManifests = @(Get-ChildItem -LiteralPath $rustOutput -Recurse -Filter '*.manifest' -File)
        foreach ($name in @($eaManifests.Name + $rustManifests.Name | Sort-Object -Unique)) {
            $expected = $eaManifests | Where-Object Name -EQ $name
            $actual = $rustManifests | Where-Object Name -EQ $name
            if (@($expected).Count -ne 1 -or @($actual).Count -ne 1) {
                Write-Host "  DIFF stream inventory $name (EA $(@($expected).Count), Rust $(@($actual).Count))"
                $same = $false
                continue
            }
            Write-Host "  STREAM $name"
            Add-Content -LiteralPath $comparisonLog -Value "STREAM $name"
            & $rust compare-linked $expected.FullName $actual.FullName *>> $comparisonLog
            if ($LASTEXITCODE -ne 0) { $same = $false }
        }
        $versionPaths = foreach ($root in @($eaOutput, $rustOutput)) {
            Get-ChildItem -LiteralPath $root -Recurse -Filter '*.version' -File |
                ForEach-Object { $_.FullName.Substring($root.Length + 1) }
        }
        foreach ($path in ($versionPaths | Sort-Object -Unique)) {
            $eaVersion = Join-Path $eaOutput $path
            $rustVersion = Join-Path $rustOutput $path
            if (!(Test-Path -LiteralPath $eaVersion -PathType Leaf) -or
                !(Test-Path -LiteralPath $rustVersion -PathType Leaf) -or
                (Get-FileHash -LiteralPath $eaVersion).Hash -ne (Get-FileHash -LiteralPath $rustVersion).Hash) {
                Write-Host "  DIFF version sidecar $path"
                $same = $false
            }
        }
    }
    if (Test-Path -LiteralPath $comparisonLog) {
        Get-Content -LiteralPath $comparisonLog | Where-Object { $_ -notlike 'STREAM *' } |
            ForEach-Object { Write-Host "    $_" }
    }
    $eaWarnings = @(Get-ReferenceWarnings $eaLog)
    $rustWarnings = @(Get-ReferenceWarnings $rustLog)
    if (($eaWarnings -join "`n") -cne ($rustWarnings -join "`n")) {
        Write-Host '  DIFF unknown-reference warnings'
        $eaWarnings | ForEach-Object { Write-Host "    EA   $_" }
        $rustWarnings | ForEach-Object { Write-Host "    Rust $_" }
        $same = $false
    }
    if ($same) { $passed++ }
}
Write-Host "Matched $passed of $($cases.Count) cases. Logs and streams are in $work"
if ($passed -ne $cases.Count) { exit 1 }
