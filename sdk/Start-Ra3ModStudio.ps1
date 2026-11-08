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
$hasRustCheck = $currentSource.Contains('echo Checking linked files with Rust') -or $currentSource.Contains('compile-static-sdk')
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
$staticCheckSource = $source
$aptUiStep = @'
case 4:
    object useRust;
    if (currentGUIData.TryGetValue("rustAptUi", out useRust) && (bool)useRust)
    {
        BuildHelper.DisplayLine("Step 4: Compiling AptUI XML with Rust");
        RunExecutableArguments argsRust = new RunExecutableArguments(Cmd);
        argsRust.Arguments = String.Format(
            "/C (@echo off) & (\"{0}\" compile-aptui \"{1}\" --data-root \"{2}\" --output \"{3}\" 2>&1)",
            RUST_EXE, Path.Combine(ModDataPath, "aptui"), SDKDirectory, Path.Combine(BuiltModDataPath, "aptui"));
        BuildHelper.RunStep(StepType.RunExecutable, argsRust);
        break;
    }
'@
$aptUiStep = ($aptUiStep.Replace('RUST_EXE', $rustExpression) -split '\r?\n') -join ($lineEnding + "`t`t`t`t")
$aptUiOption = 'modGUIData.Add(new GUIElementData("step4", "Build AptUI", GUIElementData.CheckBoxType, ScriptIndex.ModScript));'
foreach ($anchor in @('case 4:', $aptUiOption)) {
    $index = $source.IndexOf($anchor, [StringComparison]::Ordinal)
    if ($index -lt 0 -or $index -ne $source.LastIndexOf($anchor, [StringComparison]::Ordinal)) {
        throw "The AptUI step in $sourcePath has an unexpected layout."
    }
}
$source = $source.Replace('case 4:', $aptUiStep).Replace(
    $aptUiOption,
    $aptUiOption + $lineEnding + "`t`t`t" +
    'modGUIData.Add(new GUIElementData("rustAptUi", "Use Rust for AptUI", GUIElementData.CheckBoxType, ScriptIndex.ModScript));'
)
$aptUiSource = $source

$staticBuild = @'
case 6:
    BuildHelper.DisplayLine(String.Format("Step {0}: Building static data with Rust and EA fallback", stepId));
    RunExecutableArguments args = new RunExecutableArguments(Cmd);
    args.Arguments = String.Format(
        "/C (@echo off) & (cd /D \"{2}\")"
        + " && (\"{0}\" compile-static-sdk \"{1}\" --sdk-root \"{2}\" --output \"{3}\" 2>&1)"
        + " && (for %I in (\"{4}\\mod_*\") do (del \"%I\" /F /Q))"
        + " && (echo.>\"{4}\\mod.version\")",
        RUST_EXE, ModXml, SDKDirectory, BuiltModsPath, BuiltModDataPath);
    if (((bool)currentGUIData["step7"] == false) && ((bool)currentGUIData["step8"] == false))
    {
        args.Arguments += String.Format(" && (\"{0}\" \"{1}\\mod.manifest\" /f)", LoDStreamBuilder, BuiltModDataPath);
    }
    args.Arguments += " || (echo ERROR Build Static Data failed. & exit /b 1)";
    BuildHelper.RunStep(StepType.RunExecutable, args);
    break;
'@
$staticIndent = "`t`t`t`t"
$staticBuild = $staticIndent + (($staticBuild.Replace('RUST_EXE', $rustExpression) -split '\r?\n') -join ($lineEnding + $staticIndent)) + $lineEnding
$currentStep = [regex]::Match($source, '(?ms)^[\t ]*case 6:.*?(?=^[\t ]*case 7:)')
$source = $source.Remove($currentStep.Index, $currentStep.Length).Insert($currentStep.Index, $staticBuild)

if ($InstallDefault) {
    if ($hasRustCheck -and $currentSource -cne $source -and $currentSource -cne $aptUiSource -and $currentSource -cne $staticCheckSource) {
        throw "The installed Rust check script has been edited. Review $sourcePath before reinstalling."
    }
    if (-not $hasRustCheck -and (Test-Path -LiteralPath $backupPath)) {
        throw "A backup already exists at $backupPath. Review it before reinstalling."
    }

    $installedRustPath = Join-Path $SdkRoot 'tools\binary-asset-builder.exe'
    if (Test-Path -LiteralPath $installedRustPath) {
        if ((Get-FileHash -LiteralPath $installedRustPath).Hash -ne (Get-FileHash -LiteralPath $rustPath).Hash) {
            if (-not $hasRustCheck) {
                throw "A different Rust executable exists at $installedRustPath. Move it aside before installing this build."
            }
            $pendingExe = "$installedRustPath.$([Guid]::NewGuid().ToString('N')).tmp"
            try {
                [IO.File]::Copy($rustPath, $pendingExe)
                [IO.File]::Replace($pendingExe, $installedRustPath, "$installedRustPath.previous")
            } finally {
                [IO.File]::Delete($pendingExe)
            }
        }
    } else {
        [IO.File]::Copy($rustPath, $installedRustPath)
    }

    if ($currentSource -cne $source) {
        $replacementBackup = if ($hasRustCheck) { "$sourcePath.before-rust-static" } else { $backupPath }
        $pendingPath = Join-Path $SdkRoot ("defaultscript.cs." + [Guid]::NewGuid().ToString('N') + '.tmp')
        try {
            [IO.File]::WriteAllText($pendingPath, $source)
            [IO.File]::Replace($pendingPath, $sourcePath, $replacementBackup)
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
