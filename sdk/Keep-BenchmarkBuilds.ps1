[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Build,
    [Parameter(Mandatory)] [string] $BenchmarkRoot,
    [string] $PackageRoot
)

$ErrorActionPreference = 'Stop'
$benchmark = Get-Item -LiteralPath $BenchmarkRoot
if (!$benchmark.PSIsContainer -or ($benchmark.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw "Invalid benchmark root $BenchmarkRoot"
}
$root = $benchmark.FullName
$completed = Get-Item -LiteralPath $Build
if (!$completed.PSIsContainer -or $completed.Parent.FullName -ine $root -or
    ($completed.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
    !(Test-Path -LiteralPath (Join-Path $completed.FullName 'builtmods') -PathType Container)) {
    throw "Expected a generated benchmark directory directly under $root"
}
$external = if ($PackageRoot -and (Test-Path -LiteralPath $PackageRoot -PathType Container)) {
    Get-Item -LiteralPath $PackageRoot
}
if ($external -and ($external.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw "Invalid benchmark package root $PackageRoot"
}

# Serialize publication of the retention markers when multiple benchmark runners finish.
$lock = $null
for ($attempt = 0; $attempt -lt 100 -and !$lock; $attempt++) {
    try {
        $lock = [IO.File]::Open((Join-Path $root '.retention.lock'), 'OpenOrCreate', 'Write', 'None')
    } catch [IO.IOException] {
        if ($attempt -eq 99) { throw }
        Start-Sleep -Milliseconds 100
    }
}
try {
    $markerName = '.benchmark-complete'
    [IO.File]::WriteAllText((Join-Path $completed.FullName $markerName), '')
    $obsolete = Get-ChildItem -LiteralPath $root -Directory | Where-Object {
        !($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -and
        (Test-Path -LiteralPath (Join-Path $_.FullName $markerName) -PathType Leaf)
    } | Sort-Object -Property @{ Expression = {
        (Get-Item -LiteralPath (Join-Path $_.FullName $markerName) -Force).LastWriteTimeUtc
    }; Descending = $true }, Name | Select-Object -Skip 2

    foreach ($directory in $obsolete) {
        if ($directory.Parent.FullName -ine $root -or
            (Get-ChildItem -LiteralPath $directory.FullName -Recurse -Force -Attributes ReparsePoint | Select-Object -First 1)) {
            throw "Invalid build path $directory"
        }
        $logs = @(Get-ChildItem -LiteralPath $directory.FullName -File | Where-Object Extension -in '.log', '.err')
        if ($logs.Count) {
            $archive = New-Item -ItemType Directory -Force -Path (Join-Path $root "logs/$($directory.Name)")
            $logs | Copy-Item -Destination $archive.FullName -Force
        }
        if ($external) {
            $package = Get-Item -LiteralPath (Join-Path $external.FullName $directory.Name) -ErrorAction SilentlyContinue
            if ($package) {
                if (!$package.PSIsContainer -or $package.Parent.FullName -ine $external.FullName -or
                    ($package.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
                    (Get-ChildItem -LiteralPath $package.FullName -Recurse -Force -Attributes ReparsePoint | Select-Object -First 1)) {
                    throw "Invalid benchmark package path $package"
                }
                Remove-Item -LiteralPath $package.FullName -Recurse -Force
            }
        }
        Remove-Item -LiteralPath $directory.FullName -Recurse -Force
    }
} finally {
    $lock.Dispose()
}

