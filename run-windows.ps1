param(
    [switch]$Watch,
    [switch]$Offline,
    [ValidateRange(0, 65535)][int]$WebPort = 0,
    [ValidateRange(0, 65535)][int]$ScpiPort = 0,
    [ValidateRange(0, 65535)][int]$HislipPort = 0,
    [string[]]$AppArgs = @()
)

# Run the current checkout. Watch rebuilds and replaces only the app process started by this script.
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$savedPath = $env:PATH
$appProcess = $null
$projectDir = $PSScriptRoot
$targetDir = Join-Path $projectDir 'local\windows-dev-target'
$executable = Join-Path $targetDir 'debug\oos-ui.exe'
$logDir = Join-Path $projectDir 'local\windows-dev'
if ($WebPort -gt 0) { $AppArgs += @('--web-port', $WebPort.ToString()) }
if ($ScpiPort -gt 0) { $AppArgs += @('--scpi-port', $ScpiPort.ToString()) }
if ($HislipPort -gt 0) { $AppArgs += @('--hislip-port', $HislipPort.ToString()) }

function Get-SourceStamp {
    $files = @()
    foreach ($relative in @('ui\src', 'ui\assets', 'dsp\src', 'hal\src')) {
        $directory = Join-Path $projectDir $relative
        if (Test-Path -LiteralPath $directory) {
            $files += Get-ChildItem -LiteralPath $directory -File -Recurse
        }
    }
    foreach ($relative in @('ui\Cargo.toml', 'ui\Cargo.lock', 'dsp\Cargo.toml', 'dsp\Cargo.lock', 'hal\Cargo.toml', 'hal\Cargo.lock')) {
        $path = Join-Path $projectDir $relative
        if (Test-Path -LiteralPath $path) { $files += Get-Item -LiteralPath $path }
    }
    return ($files | Sort-Object FullName | ForEach-Object { '{0}|{1}|{2}' -f $_.FullName, $_.Length, $_.LastWriteTimeUtc.Ticks }) -join "`n"
}

function Stop-OwnedApp {
    if ($null -eq $script:appProcess) { return }
    $script:appProcess.Refresh()
    if (-not $script:appProcess.HasExited) {
        # The saved Process object belongs to this launcher. Never stop another OOS window by name.
        $actual = [System.IO.Path]::GetFullPath($script:appProcess.Path)
        if ($actual -ne [System.IO.Path]::GetFullPath($executable)) {
            throw 'The watched process path changed; refusing to stop it.'
        }
        Stop-Process -InputObject $script:appProcess
        $script:appProcess.WaitForExit()
    }
    $script:appProcess.Dispose()
    $script:appProcess = $null
}

function Quote-AppArgument([string]$value) {
    if ($value.Length -gt 0 -and $value -notmatch '[\s"]') { return $value }
    # Start-Process joins ArgumentList with spaces; preserve spaces, quotes and trailing backslashes.
    return '"' + ($value -replace '(\\*)"', '$1$1\"' -replace '(\\+)$', '$1$1') + '"'
}

try {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    $env:PATH = "$cargoBin;$savedPath"
    $sysroot = & rustc --print sysroot
    if ($LASTEXITCODE -ne 0) { throw 'A Windows Rust toolchain is required.' }
    $hostLine = & rustc -vV | Select-String '^host: '
    $rustHost = $hostLine.ToString().Substring(6).Trim()
    if ($rustHost -like '*windows-gnu') {
        $toolBin = Join-Path $sysroot "lib\rustlib\$rustHost\bin\self-contained"
        $env:PATH = "$toolBin;$env:PATH"
    }
    $buildArgs = @('build', '--manifest-path', (Join-Path $projectDir 'ui\Cargo.toml'), '--target-dir', $targetDir)
    if ($Offline) { $buildArgs += '--offline' }
    New-Item -ItemType Directory -Path $logDir -Force | Out-Null
    $buildLog = Join-Path $logDir 'build.log'
    $stamp = Get-SourceStamp
    $pending = $true
    $changedAt = [datetime]::UtcNow.AddSeconds(-2)
    Write-Output $(if ($Watch) { 'Watching UI, DSP, HAL and assets. Save a source file to rebuild and restart this app.' } else { 'Building and running the current checkout.' })
    while ($true) {
        if ($pending -and ([datetime]::UtcNow - $changedAt).TotalSeconds -ge 1) {
            Stop-OwnedApp
            $buildingStamp = Get-SourceStamp
            Write-Output 'Building the Windows development app...'
            & cargo @buildArgs *> $buildLog
            $buildCode = $LASTEXITCODE
            Get-Content -LiteralPath $buildLog -Tail 20
            if ($buildCode -eq 0) {
                $launch = @{ FilePath = $executable; WorkingDirectory = $projectDir; WindowStyle = 'Normal'; PassThru = $true }
                if ($AppArgs.Count -gt 0) { $launch.ArgumentList = @($AppArgs | ForEach-Object { Quote-AppArgument $_ }) }
                $script:appProcess = Start-Process @launch
                Write-Output "Running the current source (PID $($script:appProcess.Id))."
            } else {
                Write-Output "Build failed. Details: $buildLog"
                if (-not $Watch) { exit $buildCode }
                Write-Output 'Waiting for the next source edit.'
            }
            $stamp = Get-SourceStamp
            $pending = $stamp -ne $buildingStamp
            $changedAt = [datetime]::UtcNow
        }
        if (-not $Watch) {
            if ($null -ne $script:appProcess) { $script:appProcess.WaitForExit() }
            break
        }
        Start-Sleep -Milliseconds 400
        $current = Get-SourceStamp
        if ($current -ne $stamp) {
            $stamp = $current
            $pending = $true
            $changedAt = [datetime]::UtcNow
        }
    }
} finally {
    Stop-OwnedApp
    $env:PATH = $savedPath
}
