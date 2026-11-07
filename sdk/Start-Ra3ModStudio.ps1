[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $SdkRoot,

    [switch] $PrepareOnly
)

$ErrorActionPreference = 'Stop'
$SdkRoot = (Get-Item -LiteralPath $SdkRoot).FullName
$studio = Get-Item -LiteralPath (Join-Path $SdkRoot 'EALAModStudio.exe')
$repository = Split-Path -Parent $PSScriptRoot
$rustPath = Join-Path $repository 'target\release\binary-asset-builder.exe'
if (-not (Test-Path -LiteralPath $rustPath -PathType Leaf)) {
    throw 'Build the Rust executable with cargo build --release before starting ModStudio.'
}

$sourcePath = Join-Path $SdkRoot 'defaultscript.cs'
$source = [IO.File]::ReadAllText($sourcePath)
$steps = [regex]::Matches($source, '(?ms)^[\t ]*case 6:.*?(?=^[\t ]*case 7:)')
if ($steps.Count -ne 1) {
    throw "Could not locate the Build Static Data step in $sourcePath."
}

$step = $steps[0]
$lineEnding = if ($step.Value.Contains("`r`n")) { "`r`n" } else { "`n" }
$rustLiteral = '@"' + $rustPath.Replace('"', '""') + '"'
$checkCommands = @'
+ " && (echo Checking linked files with Rust)"
+ " && (\"{6}\" check-linked \"{1}\\mod.manifest\" 2>&1)"
+ " && (echo.>>\"{1}\\mod.version\")",
'@
$indent = "`t`t`t`t`t`t"
$checkCommands = ($checkCommands -split '\r?\n') -join ($lineEnding + $indent)

$replacements = [ordered]@{
    '/res:true' = '/res:false'
    'SageXml\")"' = 'SageXml\" 2>&1)"'
    '+ " & (if exist \"{1}\\mod.manifest\" (echo.>>\"{1}\\mod.version\"))",' = $checkCommands
    'BinaryAssetBuilder, ModXml, BuiltModsPath, Mod);' = "BinaryAssetBuilder, ModXml, BuiltModsPath, Mod, $rustLiteral);"
    'String.Format(" & (\"{0}\" \"{1}\\mod.manifest\" /f)"' = 'String.Format(" && (\"{0}\" \"{1}\\mod.manifest\" /f)"'
    'BuildHelper.RunStep(StepType.RunExecutable, args);' = (
        'args.Arguments += " || (echo ERROR Build Static Data failed. & exit /b 1)";' +
        $lineEnding + "`t`t`t`t`t" + 'BuildHelper.RunStep(StepType.RunExecutable, args);'
    )
}
$updatedStep = $step.Value
foreach ($original in $replacements.Keys) {
    $index = $updatedStep.IndexOf($original, [StringComparison]::Ordinal)
    if ($index -lt 0 -or $index -ne $updatedStep.LastIndexOf($original, [StringComparison]::Ordinal)) {
        throw "The Build Static Data step in $sourcePath has an unexpected layout."
    }
    $updatedStep = $updatedStep.Replace($original, $replacements[$original])
}

$destination = Join-Path $repository 'target\sdk-modstudio'
New-Item -ItemType Directory -Force -Path $destination | Out-Null
$scriptPath = Join-Path $destination 'rust-check.cs'
$source = $source.Remove($step.Index, $step.Length).Insert($step.Index, $updatedStep)
[IO.File]::WriteAllText($scriptPath, $source)

if ($PrepareOnly) {
    Write-Output $scriptPath
    return
}

Start-Process -FilePath $studio.FullName -ArgumentList ('-script "{0}"' -f $scriptPath) -WorkingDirectory $SdkRoot -WindowStyle Normal
