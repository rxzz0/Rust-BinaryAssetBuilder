[CmdletBinding(DefaultParameterSetName = 'Launch')]
param(
    [Parameter(Mandatory)]
    [string] $SdkRoot,

    [Parameter(ParameterSetName = 'Prepare')]
    [switch] $PrepareOnly,

    [Parameter(ParameterSetName = 'Install')]
    [switch] $InstallDefault
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
$backupPath = "$sourcePath.before-rust-check"
$currentSource = [IO.File]::ReadAllText($sourcePath)
$hasRustCheck = $currentSource.Contains('echo Checking linked files with Rust')
$source = if ($hasRustCheck) { [IO.File]::ReadAllText($backupPath) } else { $currentSource }
$steps = [regex]::Matches($source, '(?ms)^[\t ]*case 6:.*?(?=^[\t ]*case 7:)')
if ($steps.Count -ne 1) {
    throw "Could not locate the Build Static Data step in $sourcePath."
}

$step = $steps[0]
$lineEnding = if ($step.Value.Contains("`r`n")) { "`r`n" } else { "`n" }
$rustExpression = if ($InstallDefault) {
    'Path.Combine(SDKDirectory, @"tools\binary-asset-builder.exe")'
} else {
    '@"' + $rustPath.Replace('"', '""') + '"'
}
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
    'BinaryAssetBuilder, ModXml, BuiltModsPath, Mod);' = "BinaryAssetBuilder, ModXml, BuiltModsPath, Mod, $rustExpression);"
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

$source = $source.Remove($step.Index, $step.Length).Insert($step.Index, $updatedStep)
if ($InstallDefault) {
    if ($hasRustCheck -and $currentSource -cne $source) {
        throw "The installed Rust check script has been edited. Review $sourcePath before reinstalling."
    }
    if (-not $hasRustCheck -and (Test-Path -LiteralPath $backupPath)) {
        throw "A backup already exists at $backupPath. Review it before reinstalling."
    }

    $installedRustPath = Join-Path $SdkRoot 'tools\binary-asset-builder.exe'
    if (Test-Path -LiteralPath $installedRustPath) {
        if ((Get-FileHash -LiteralPath $installedRustPath).Hash -ne (Get-FileHash -LiteralPath $rustPath).Hash) {
            throw "A different Rust executable exists at $installedRustPath. Move it aside before installing this build."
        }
    } else {
        [IO.File]::Copy($rustPath, $installedRustPath)
    }

    if (-not $hasRustCheck) {
        $pendingPath = Join-Path $SdkRoot ("defaultscript.cs." + [Guid]::NewGuid().ToString('N') + '.tmp')
        try {
            [IO.File]::WriteAllText($pendingPath, $source)
            [IO.File]::Replace($pendingPath, $sourcePath, $backupPath)
        } finally {
            [IO.File]::Delete($pendingPath)
        }
    }
    Write-Output $sourcePath
    return
}

$destination = Join-Path $repository 'target\sdk-modstudio'
New-Item -ItemType Directory -Force -Path $destination | Out-Null
$scriptPath = Join-Path $destination 'rust-check.cs'
[IO.File]::WriteAllText($scriptPath, $source)

if ($PrepareOnly) {
    Write-Output $scriptPath
    return
}

Start-Process -FilePath $studio.FullName -ArgumentList ('-script "{0}"' -f $scriptPath) -WorkingDirectory $SdkRoot -WindowStyle Normal
