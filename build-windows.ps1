param(
    [switch]$Run,
    [switch]$Offline
)

$ErrorActionPreference = 'Stop'
$savedPath = $env:PATH
try {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    $env:PATH = "$cargoBin;$savedPath"
    $sysroot = & rustc --print sysroot
    if ($LASTEXITCODE -ne 0) { throw 'Rust is required. Install a Windows Rust toolchain first.' }
    $hostLine = & rustc -vV | Select-String '^host: '
    $rustHost = $hostLine.ToString().Substring(6).Trim()
    if ($rustHost -like '*windows-gnu') {
        $toolBin = Join-Path $sysroot "lib\rustlib\$rustHost\bin\self-contained"
        $env:PATH = "$toolBin;$env:PATH"
    }
    $buildArgs = @('build', '--release', '--manifest-path', (Join-Path $PSScriptRoot 'ui\Cargo.toml'))
    if ($Offline) { $buildArgs += '--offline' }
    & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw 'The Windows build failed.' }
    $executable = Join-Path $PSScriptRoot 'ui\target\release\oos-ui.exe'
    Write-Output "Windows app: $executable"
    if ($Run) {
        Start-Process -FilePath $executable -WorkingDirectory $PSScriptRoot -WindowStyle Normal
    }
} finally {
    $env:PATH = $savedPath
}
