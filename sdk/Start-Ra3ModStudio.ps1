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

$SdkRoot = (Resolve-Path -LiteralPath $SdkRoot).ProviderPath
$studio = Get-Item -LiteralPath (Join-Path $SdkRoot 'EALAModStudio.exe')
$repository = Split-Path -Parent $PSScriptRoot
$rustPath = Join-Path $repository 'target\release\binary-asset-builder.exe'
if (!(Test-Path -LiteralPath $rustPath -PathType Leaf)) {
    throw "Rust compiler missing at $rustPath. Keep the package together or run cargo build --release."
}

$scriptPath = Join-Path $SdkRoot 'defaultscript.cs'
$bundledScript = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'defaultscript.cs'))
$currentScript = [IO.File]::ReadAllText($scriptPath)

# Version defaults are optional. The SDK script's build steps are never used.
$versions = [regex]::new('(?m)(^[ \t]*static string (DEFAULT_(?:GAME|MOD)_VERSION) = ")[A-Za-z0-9._-]+(";[ \t]*\r?$)')
$script = $versions.Replace($bundledScript, [System.Text.RegularExpressions.MatchEvaluator]{
    param($match)
    $existing = @($versions.Matches($currentScript) | Where-Object { $_.Groups[2].Value -ceq $match.Groups[2].Value })
    if ($existing.Count -eq 1) { $existing[0].Value } else { $match.Value }
})

function Replace-SdkFile([string] $source, [string] $destination, [AllowNull()][string] $backup) {
    $readOnly = [IO.FileAttributes]::ReadOnly
    $cleared = @{}
    try {
        foreach ($path in @($destination, $backup)) {
            if (!$path -or ![IO.File]::Exists($path)) { continue }
            $attributes = [IO.File]::GetAttributes($path)
            if ($attributes -band $readOnly) {
                [IO.File]::SetAttributes($path, [IO.FileAttributes]($attributes -band (-bnot [int]$readOnly)))
                $cleared[$path] = $attributes
            }
        }
        if ($backup) {
            [IO.File]::Replace($source, $destination, $backup)
        } else {
            [IO.File]::Replace($source, $destination, [NullString]::Value)
        }
    } catch {
        foreach ($path in $cleared.Keys) {
            if ([IO.File]::Exists($path)) {
                [IO.File]::SetAttributes($path, $cleared[$path])
            }
        }
        throw "Could not replace '$destination'. $($_.Exception.Message)"
    }
}

if ($InstallDefault) {
    $backupDir = Join-Path $SdkRoot 'defaultscript'
    [void][IO.Directory]::CreateDirectory($backupDir)
    $lock = [IO.File]::Open((Join-Path $backupDir '.install.lock'), 'OpenOrCreate', 'Write', 'None')
    try {
        # Detect a concurrent update before overwriting the SDK script.
        if ([IO.File]::ReadAllText($scriptPath) -cne $currentScript) {
            throw "The SDK script changed during installation. Run the installer again."
        }

        $original = Join-Path $backupDir 'defaultscript.cs'
        $previous = Join-Path $backupDir 'defaultscript.previous.cs'
        $legacy = "$scriptPath.before-rust-check"
        if (!(Test-Path -LiteralPath $original) -and $currentScript.Contains('compile-static-sdk') -and
            (Test-Path -LiteralPath $legacy -PathType Leaf)) {
            [IO.File]::Copy($legacy, $original)
        }

        $installedRust = Join-Path $SdkRoot 'tools\binary-asset-builder.exe'
        [void][IO.Directory]::CreateDirectory((Split-Path -Parent $installedRust))
        $hadRust = [IO.File]::Exists($installedRust)
        $changeRust = !$hadRust -or (Get-FileHash -LiteralPath $installedRust).Hash -ne (Get-FileHash -LiteralPath $rustPath).Hash
        $nonce = [guid]::NewGuid().ToString('N')
        $pendingRust = "$installedRust.$nonce.tmp"
        $pendingScript = "$scriptPath.$nonce.tmp"
        $previousRust = "$installedRust.previous"
        $rustUpdated = $false
        try {
            if ($currentScript -cne $script) {
                [IO.File]::WriteAllText($pendingScript, $script)
            }
            if ($changeRust) {
                [IO.File]::Copy($rustPath, $pendingRust)
                if ($hadRust) {
                    Replace-SdkFile $pendingRust $installedRust $previousRust
                } else {
                    [IO.File]::Move($pendingRust, $installedRust)
                }
                $rustUpdated = $true
            }
            if ($currentScript -cne $script) {
                $backup = if ([IO.File]::Exists($original)) { $previous } else { $original }
                Replace-SdkFile $pendingScript $scriptPath $backup
            } elseif (![IO.File]::Exists($original)) {
                [IO.File]::Copy($scriptPath, $original)
            }
        } catch {
            if ($rustUpdated) {
                if ($hadRust) {
                    Replace-SdkFile $previousRust $installedRust $null
                } else {
                    [IO.File]::Delete($installedRust)
                }
            }
            throw
        } finally {
            [IO.File]::Delete($pendingRust)
            [IO.File]::Delete($pendingScript)
        }
    } finally {
        $lock.Dispose()
    }
    Write-Output $scriptPath
    return
}

$script = $script.Replace('Path.Combine(SDKDirectory, @"tools\binary-asset-builder.exe")', '@"' + $rustPath.Replace('"', '""') + '"')
$destination = Join-Path $repository 'target\sdk-modstudio'
[void][IO.Directory]::CreateDirectory($destination)
$preparedPath = Join-Path $destination 'rust-check.cs'
[IO.File]::WriteAllText($preparedPath, $script)

if ($PrepareOnly) {
    Write-Output $preparedPath
    return
}

Start-Process -FilePath $studio.FullName -ArgumentList ('-script "{0}"' -f $preparedPath) -WorkingDirectory $SdkRoot -WindowStyle Normal
