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

function Add-RustWarningOption([string] $script) {
    $checkbox = [regex]::new('(?m)^([\t ]*)(modGUIData\.Add\(new GUIElementData\("step8",[^\r\n]*\);)')
    $executeStep = [regex]::new('(?m)^[\t ]*private void executeModBuildStep\(int stepId\)')
    $runRust = [regex]::new('BuildHelper\.RunStep\(StepType\.RunExecutable, (argsRust[A-Z]\w*|argsBig)\);')
    if ($checkbox.Matches($script).Count -ne 1 -or $executeStep.Matches($script).Count -ne 1 -or $runRust.Matches($script).Count -ne 6) {
        throw 'Could not add the Rust warnings option to this Mod Studio script.'
    }
    $lineEnd = if ($script.Contains([string][char]13 + [char]10)) { [string][char]13 + [char]10 } else { [string][char]10 }
    $script = $checkbox.Replace($script, '$0' + $lineEnd + '            modGUIData.Add(new GUIElementData("hideRustWarnings", "Hide Rust warnings", GUIElementData.CheckBoxType, ScriptIndex.ModScript));')
    $script = $runRust.Replace($script, 'RunRustBuildStep($1);')
    $helper = @"
        private void RunRustBuildStep(RunExecutableArguments args)
        {
            object hideWarnings;
            if (currentGUIData.TryGetValue("hideRustWarnings", out hideWarnings) && hideWarnings is bool && (bool)hideWarnings)
            {
                args.Arguments = args.Arguments.Insert(3, "(set \"BAB_NO_WARNINGS=1\") & ");
            }
            BuildHelper.RunStep(StepType.RunExecutable, args);
        }
"@
    $helper = ($helper -split '\r?\n') -join $lineEnd
    return $executeStep.Replace($script, $helper + $lineEnd + $lineEnd + '$0')
}

function Add-CompilerOption([string] $script, [string] $original) {
    $methods = [regex]::new('(?m)^([\t ]*)private void executeModBuildStep\(int stepId\)\s*\{')
    $cases = [regex]::Match($original, '(?ms)^[\t ]*case 4:.*?(?=^[\t ]*case 11:)')
    $mapOption = [regex]::Match($original, '(?m)^[\t ]*modGUIData\.Add\(new GUIElementData\("step9",[^\r\n]*')
    $options = [regex]::new('(?m)^([\t ]*)(modGUIData\.Add\(new GUIElementData\("hideRustWarnings",[^\r\n]*)')
    $skipMaps = 'if (stepId == 9) return false;'
    if ($methods.Matches($script).Count -ne 1 -or !$cases.Success -or !$mapOption.Success -or
        $options.Matches($script).Count -ne 1 -or !$script.Contains($skipMaps)) {
        throw 'Could not add the compiler selector to this ModStudio script.'
    }
    $lineEnd = if ($script.Contains("`r`n")) { "`r`n" } else { "`n" }
    $selector = 'modGUIData.Add(new GUIElementData("useEaCompiler", "Use original EA compiler", GUIElementData.CheckBoxType, ScriptIndex.ModScript));'
    $script = $options.Replace($script, [System.Text.RegularExpressions.MatchEvaluator]{
        param($match)
        $match.Groups[1].Value + $selector + $lineEnd + $match.Value + $lineEnd + $mapOption.Value
    })
    $script = $script.Replace($skipMaps, 'if (stepId == 9 && !UseEaCompiler()) return false;')
    $helper = @'
        private bool UseEaCompiler()
        {
            object setting;
            return currentGUIData.TryGetValue("useEaCompiler", out setting) && setting is bool && (bool)setting;
        }

        private void ExecuteEaBuildStep(int stepId)
        {
            switch (stepId)
            {
EA_CASES
            }
        }
'@
    $eaCases = $cases.Value.TrimEnd()
    foreach ($tool in @{
        BinaryAssetBuilder = 'binaryassetbuilder.exe'; LoDStreamBuilder = 'lodstreambuilder.exe'
        AssetMerger = 'assetmerger.exe'; StreamSorter = 'streamsorter.exe'
        HashFix = 'hashfix.exe'; AssetResolver = 'modassetresolver.exe'; MakeBig = 'makebig.exe'
    }.GetEnumerator()) {
        $eaCases = [regex]::Replace($eaCases, '\b' + $tool.Key + '\b', ('Path.Combine(SDKDirectory, @"tools\' + $tool.Value + '")'))
    }
    $helper = ($helper.Replace('EA_CASES', $eaCases) -split '\r?\n') -join $lineEnd
    $gate = @'
            if (stepId >= 4 && stepId <= 10 && UseEaCompiler())
            {
                ExecuteEaBuildStep(stepId);
                return;
            }
'@
    $gate = ($gate -split '\r?\n') -join $lineEnd
    return $methods.Replace($script, [System.Text.RegularExpressions.MatchEvaluator]{
        param($match)
        $helper + $lineEnd + $lineEnd + $match.Value + $lineEnd + $gate
    })
}

function Move-CompilerOptions([string] $script) {
    foreach ($placement in @(
        @{ Option = 'useEaCompiler'; Anchor = 'step1'; Before = $true }
        @{ Option = 'hideRustWarnings'; Anchor = 'step11'; Before = $false }
    )) {
        $option = [regex]::new('(?m)^[\t ]*modGUIData\.Add\(new GUIElementData\("' + $placement.Option + '",[^\r\n]*\r?\n')
        $anchor = [regex]::new('(?m)^[\t ]*modGUIData\.Add\(new GUIElementData\("' + $placement.Anchor + '",[^\r\n]*\r?\n')
        if ($option.Matches($script).Count -ne 1 -or $anchor.Matches($script).Count -ne 1) {
            throw "Could not position the ModStudio option $($placement.Option)."
        }
        $line = $option.Match($script).Value
        $script = $option.Replace($script, '')
        $position = $anchor.Match($script)
        $index = $position.Index
        if (!$placement.Before) { $index += $position.Length }
        $script = $script.Insert($index, $line)
    }
    return $script
}

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
if ($hasRustCheck -and !(Test-Path -LiteralPath $backupPath)) {
    $backupPath = Join-Path $SdkRoot 'defaultscriptC.cs'
}
$source = if ($hasRustCheck) { [IO.File]::ReadAllText($backupPath) } else { $currentSource }
$originalSource = $source
if ($hasRustCheck) {
    $versionDefaults = [regex]::new('(?m)^[\t ]*static string DEFAULT_(?:GAME|MOD)_VERSION\s*=\s*"[^"\r\n]*";')
    $source = $versionDefaults.Replace($source, [System.Text.RegularExpressions.MatchEvaluator]{
        param($match)
        $name = [regex]::Match($match.Value, 'DEFAULT_(?:GAME|MOD)_VERSION').Value
        $installed = $versionDefaults.Matches($currentSource) | Where-Object { $_.Value.Contains($name) }
        if (@($installed).Count -eq 1) { $installed.Value } else { $match.Value }
    })
}
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
$hybridSource = $source

$packBuild = @'
case 10:
    BuildHelper.DisplayLine(String.Format("Step {0}: Creating BIG files with Rust", stepId));
    RunExecutableArguments argsBig = new RunExecutableArguments(Cmd);
    argsBig.Arguments = String.Format(
        "/C (@echo off) & (cd /D \"{0}\")"
        + " && (if exist \"{2}\" (\"{1}\" pack-big \"{2}\" --output \"{3}\\{4}\" 2>&1))"
        + " && (if exist \"{5}\" (\"{1}\" pack-big \"{5}\" --output \"{3}\\{6}\" 2>&1))"
        + " || (echo ERROR Creating BIG files failed. & exit /b 1)",
        SDKDirectory, RUST_EXE, BuiltModPath, ModInstallPath, ModBigStreams, ModAdditionalFilesPath, ModBigMisc);
    BuildHelper.RunStep(StepType.RunExecutable, argsBig);
    break;
'@
$packBuild = $staticIndent + (($packBuild.Replace('RUST_EXE', $rustExpression) -split '\r?\n') -join ($lineEnding + $staticIndent)) + $lineEnding
$packSteps = [regex]::Matches($source, '(?ms)^[\t ]*case 10:.*?(?=^[\t ]*case 11:)')
if ($packSteps.Count -ne 1) {
    throw "Could not locate the BIG packaging step in $sourcePath."
}
$packStep = $packSteps[0]
$source = $source.Remove($packStep.Index, $packStep.Length).Insert($packStep.Index, $packBuild)
$packSource = $source

function Set-ModBuildStep([string] $script, [int] $id, [int] $next, [string] $body) {
    $steps = [regex]::Matches($script, "(?ms)^[\t ]*case ${id}:.*?(?=^[\t ]*case ${next}:)")
    if ($steps.Count -ne 1) {
        throw "Could not locate ModStudio build step $id in $sourcePath."
    }
    $step = $steps[0]
    $text = $staticIndent + (($body.Replace('RUST_EXE', $rustExpression) -split '\r?\n') -join ($lineEnding + $staticIndent)) + $lineEnding
    $script.Remove($step.Index, $step.Length).Insert($step.Index, $text)
}

$source = [regex]::Replace($source, '(?m)^[\t ]*modGUIData\.Add\(new GUIElementData\("rustAptUi".*\r?\n', '')
$source = [regex]::Replace($source, '(?m)^[\t ]*(?:string (?:BinaryAssetBuilder|MakeBig);|(?:BinaryAssetBuilder|MakeBig) = .*;)\r?\n', '')
$source = Set-ModBuildStep $source 4 5 @'
case 4:
    BuildHelper.DisplayLine("Step 4: Compiling AptUI XML with Rust");
    RunExecutableArguments argsRustApt = new RunExecutableArguments(Cmd);
    argsRustApt.Arguments = String.Format(
        "/C (@echo off) & (if exist \"{1}\" (\"{0}\" compile-aptui \"{1}\" --data-root \"{2}\" --output \"{3}\" 2>&1))",
        RUST_EXE, Path.Combine(ModDataPath, "aptui"), SDKDirectory, Path.Combine(BuiltModDataPath, "aptui"));
    BuildHelper.RunStep(StepType.RunExecutable, argsRustApt);
    break;
'@
$source = Set-ModBuildStep $source 5 6 @'
case 5:
    BuildHelper.DisplayLine("Step 5: Compiling global XML with Rust");
    RunExecutableArguments argsRustGlobal = new RunExecutableArguments(Cmd);
    argsRustGlobal.Arguments = String.Format(
        "/C (@echo off) & (if exist \"{1}\" (\"{0}\" compile-global-sdk \"{1}\" --sdk-root \"{2}\" --output \"{3}\" 2>&1))",
        RUST_EXE, Path.Combine(ModDataPath, "additionalmaps"), SDKDirectory, BuiltModsPath);
    BuildHelper.RunStep(StepType.RunExecutable, argsRustGlobal);
    break;
'@
$source = Set-ModBuildStep $source 6 7 @'
case 6:
    BuildHelper.DisplayLine("Step 6: Compiling static XML with Rust");
    RunExecutableArguments argsRustStatic = new RunExecutableArguments(Cmd);
    argsRustStatic.Arguments = String.Format(
        "/C (@echo off) & (\"{0}\" compile-static-sdk \"{1}\" --sdk-root \"{2}\" --output \"{3}\" 2>&1)",
        RUST_EXE, ModXml, SDKDirectory, BuiltModsPath);
    if (!(bool)currentGUIData["step7"] && !(bool)currentGUIData["step8"])
    {
        argsRustStatic.Arguments += String.Format(" && (\"{0}\" \"{1}\\mod.manifest\" /f)", LoDStreamBuilder, BuiltModDataPath);
    }
    BuildHelper.RunStep(StepType.RunExecutable, argsRustStatic);
    break;
'@
$source = Set-ModBuildStep $source 9 10 @'
case 9:
    BuildHelper.DisplayLine("Step 9: Compiling official maps with Rust");
    RunExecutableArguments argsRustMaps = new RunExecutableArguments(Cmd);
    argsRustMaps.Arguments = "/C echo ERROR The Rust official map compiler is still missing. & exit /b 1";
    BuildHelper.RunStep(StepType.RunExecutable, argsRustMaps);
    break;
'@
$nativeSource = $source
$source = [regex]::Replace($source, '(?m)^[\t ]*(?:string (?:HashFix|AssetResolver);|(?:HashFix|AssetResolver) = .*;)\r?\n', '')
$source = Set-ModBuildStep $source 8 9 @'
case 8:
    BuildHelper.DisplayLine("Step 8: Fixing linked assets with Rust");
    RunExecutableArguments argsRustFix = new RunExecutableArguments(Cmd);
    argsRustFix.Arguments = String.Format(
        "/C (@echo off) & (\"{0}\" fix-assets \"{1}\" --references \"{2}\" --allow-missing-references 2>&1)"
        + " || (echo ERROR Fix Assets failed. & exit /b 1)",
        RUST_EXE, Path.Combine(BuiltModDataPath, "mod.manifest"), BuiltModsPath);
    BuildHelper.RunStep(StepType.RunExecutable, argsRustFix);
    break;
'@
$fixSource = $source

# ModStudio's process-exit callback only fails a step after reading "Critical:".
function Set-CriticalOnFailure([string] $script, [string] $arguments, [string] $message) {
    $call = 'BuildHelper.RunStep(StepType.RunExecutable, ' + $arguments + ');'
    $index = $script.IndexOf($call, [StringComparison]::Ordinal)
    if ($index -lt 0 -or $index -ne $script.LastIndexOf($call, [StringComparison]::Ordinal)) {
        throw "Could not locate the Rust build command for $message."
    }
    $failure = $arguments + '.Arguments += " || (echo Critical: ' + $message + ' failed. & exit /b 1)";'
    $script.Insert($index, $failure + $lineEnding + $staticIndent + '    ')
}

$source = Set-CriticalOnFailure $source 'argsRustApt' 'Build AptUI'
$source = Set-CriticalOnFailure $source 'argsRustGlobal' 'Build Global Data'
$source = Set-CriticalOnFailure $source 'argsRustStatic' 'Build Static Data'
$source = $source.Replace('echo ERROR Fix Assets failed.', 'echo Critical: Fix Assets failed.')
$source = $source.Replace('echo ERROR The Rust official map compiler is still missing.', 'echo Critical: The Rust official map compiler is still missing.')
$source = $source.Replace('echo ERROR Creating BIG files failed.', 'echo Critical: Creating BIG files failed.')
$source = $source.Replace('String.Format("Build failed on step {0}", BuildHelper.CurrentStep-1)', 'String.Format("Build failed on step {0}", stepId)')

$legacySource = $source
$source = $source.Replace(
    'String.Format(" && (\"{0}\" \"{1}\\mod.manifest\" /f)", LoDStreamBuilder, BuiltModDataPath)',
    ('String.Format(" && (\"{0}\" fix-assets \"{1}\" --references \"{2}\" --allow-missing-references 2>&1)", ' + $rustExpression + ', Path.Combine(BuiltModDataPath, "mod.manifest"), BuiltModsPath)')
)
$source = Set-ModBuildStep $source 7 8 @'
case 7:
{
    BuildHelper.DisplayLine("Step 7: Merging precompiled assets with Rust");
    RunExecutableArguments argsRustMerge = new RunExecutableArguments(Cmd);
    string rustMerge = RUST_EXE;
    string modManifest = Path.Combine(BuiltModDataPath, "mod.manifest");
    const string mergeFormat = "(\"{0}\" merge-assets \"{1}\" --assets \"{2}\" 2>&1 || (echo Critical: Merge Assets failed. & exit /b 1))";
    argsRustMerge.Arguments = String.Format(
        "/C (@echo off) & (if exist \"{0}\" if exist \"{1}\" {2})",
        modManifest, Path.Combine(ModAssetsPath, "mod"),
        String.Format(mergeFormat, rustMerge, modManifest, Path.Combine(ModAssetsPath, "mod")));
    foreach (string group in new string[] { "additionalmaps", "aptui" })
    {
        string pattern = group == "additionalmaps" ? "mapmetadata_*.manifest" : "*.manifest";
        string assets = Path.Combine(Path.Combine(ModAssetsPath, group), "%~nI");
        argsRustMerge.Arguments += String.Format(
            " & (for %I in (\"{0}\") do (if exist \"%~fI\" if exist \"{1}\" {2}))",
            Path.Combine(Path.Combine(BuiltModDataPath, group), pattern), assets,
            String.Format(mergeFormat, rustMerge, "%~fI", assets));
    }
    if (!(bool)currentGUIData["step8"])
    {
        argsRustMerge.Arguments += String.Format(
            " & (if exist \"{1}\" (\"{0}\" fix-assets \"{1}\" --references \"{2}\" --allow-missing-references 2>&1 || (echo Critical: Fix Assets failed. & exit /b 1)))",
            rustMerge, modManifest, BuiltModsPath);
    }
    BuildHelper.RunStep(StepType.RunExecutable, argsRustMerge);
    break;
}
'@
$source = [regex]::Replace($source, '(?m)^[\t ]*(?:string (?:AssetMerger|StreamSorter|LoDStreamBuilder);|(?:AssetMerger|StreamSorter|LoDStreamBuilder) = .*;)\r?\n', '')

$source = Set-ModBuildStep $source 10 11 @'
case 10:
    BuildHelper.DisplayLine(String.Format("Step {0}: Creating BIG files with Rust", stepId));
    RunExecutableArguments argsBig = new RunExecutableArguments(Cmd);
    argsBig.Arguments = String.Format(
        "/C (@echo off)"
        + " && (if exist \"{1}\" (\"{0}\" pack-big \"{1}\" --output \"{2}\\{3}\" 2>&1))"
        + " && (if exist \"{4}\" (\"{0}\" pack-big \"{4}\" --output \"{2}\\{5}\" 2>&1))"
        + " || (echo ERROR Creating BIG files failed. & exit /b 1)",
        RUST_EXE, BuiltModPath, ModInstallPath, ModBigStreams, ModAdditionalFilesPath, ModBigMisc);
    BuildHelper.RunStep(StepType.RunExecutable, argsBig);
    break;
'@

$preMapsSource = $source
$source = Set-ModBuildStep $source 9 10 @'
case 9:
{
    BuildHelper.DisplayLine("Step 9: Compiling official maps with Rust");
    RunExecutableArguments argsRustMaps = new RunExecutableArguments(Cmd);
    const string mapsFolder = @"Data\Maps\Official";
    argsRustMaps.Arguments = String.Format(
        "/C (@echo off) & (\"{0}\" compile-maps-sdk \"{1}\" --sdk-root \"{2}\" --references \"{3}\" --output \"{4}\" 2>&1)"
        + " || (echo Critical: Build Official Maps failed. & exit /b 1)",
        RUST_EXE, Path.Combine(Path.Combine(ModPath, "Misc"), mapsFolder), SDKDirectory, BuiltModsPath,
        Path.Combine(ModAdditionalFilesPath, mapsFolder));
    BuildHelper.RunStep(StepType.RunExecutable, argsRustMaps);
    break;
}
'@
$mapsSource = $source
$source = Set-ModBuildStep $source 9 10 @'
case 9:
    onStepSuccess(stepId);
    break;
'@
$source = [regex]::Replace($source, '(?m)^[\t ]*modGUIData\.Add\(new GUIElementData\("step9",[^\r\n]*\r?\n', '')
$skipMaps = [regex]::new('(?m)^([\t ]*)if \(currentGUIData\.ContainsKey\("step" \+ stepId\)\)')
if ($skipMaps.Matches($source).Count -ne 1) {
    throw "Could not locate the ModStudio step selection in $sourcePath."
}
$source = $skipMaps.Replace($source, '${1}if (stepId == 9) return false;' + $lineEnding + '$0')

$beforeWarningOption = $source
$source = Add-RustWarningOption $source
$warningSource = $source
$source = Add-CompilerOption $source $originalSource
$beforeOptionMove = $source
$source = Move-CompilerOptions $source

if ($InstallDefault) {
    $knownSources = @($source, $beforeOptionMove, $warningSource, $beforeWarningOption, $mapsSource, $preMapsSource, $legacySource, $fixSource, $nativeSource, $packSource, $hybridSource, $aptUiSource, $staticCheckSource)
    $knownSources += @($knownSources | ForEach-Object { $_.Replace(' --allow-missing-references 2>&1', ' 2>&1') })
    if ($hasRustCheck -and $knownSources -cnotcontains $currentSource) {
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
        $replacementBackup = if ($hasRustCheck) { "$sourcePath.before-rust-critical" } else { $backupPath }
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
