param(
    [Parameter(Mandatory)][string]$SdkRoot,
    [Parameter(Mandatory)][string]$RustCompiler,
    [Parameter(Mandatory)][string]$InputXml,
    [Parameter(Mandatory)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$sdk = (Resolve-Path -LiteralPath $SdkRoot).Path
$rust = (Resolve-Path -LiteralPath $RustCompiler).Path
$inputFile = Get-Item -LiteralPath $InputXml
if ($inputFile.PSIsContainer -or $inputFile.Extension -ne '.xml') {
    throw 'InputXml must be an XML file'
}
if (Test-Path -LiteralPath $OutputDirectory) {
    throw 'OutputDirectory must be a new directory'
}
$eaTools = Join-Path $sdk 'tools'
$ea = (Resolve-Path -LiteralPath (Join-Path $eaTools 'BinaryAssetBuilder.exe')).Path
$work = (New-Item -ItemType Directory -Path $OutputDirectory).FullName
$fixture = Join-Path $work $inputFile.Name
Copy-Item -LiteralPath $inputFile.FullName -Destination $fixture

Push-Location -LiteralPath $eaTools
try {
    & $ea $fixture "/dr:$work" "/od:$work" "/scd:$work" /bc:false /poe:false
    if ($LASTEXITCODE) { throw "EA fixture compile failed with exit code $LASTEXITCODE" }
} finally {
    Pop-Location
}

$manifest = Join-Path $work ($inputFile.BaseName + '.manifest')
$assets = Join-Path $work ($inputFile.BaseName + '\assets')
& $rust verify-ea-batch $fixture $manifest $assets
exit $LASTEXITCODE
