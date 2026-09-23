param(
    [Parameter(Mandatory=$true)][int]$OldPid,
    [Parameter(Mandatory=$true)][string]$Installer,
    [Parameter(Mandatory=$true)][string]$AppExe,
    [Parameter(Mandatory=$true)][string]$LaunchMarker
)

$ErrorActionPreference = 'Stop'
$log = Join-Path (Split-Path -Parent $Installer) 'install-update.log'
function Log([string]$Message) {
    Add-Content -LiteralPath $log -Value "$(Get-Date -Format o) $Message"
}

try {
    Set-Content -LiteralPath $LaunchMarker -Value $PID -Encoding ascii
    Log "Waiting for DBX process $OldPid to exit"
    $deadline = (Get-Date).AddSeconds(90)
    while (Get-Process -Id $OldPid -ErrorAction SilentlyContinue) {
        if ((Get-Date) -gt $deadline) { throw 'Timed out waiting for DBX to exit' }
        Start-Sleep -Milliseconds 300
    }
    if (-not (Test-Path -LiteralPath $Installer -PathType Leaf)) { throw 'Installer is missing' }
    Log "Installing $Installer"
    # /R would let NSIS start DBX itself; this independent helper owns relaunch.
    $process = Start-Process -FilePath $Installer -ArgumentList @('/S', '/UPDATE') -PassThru -Wait -WindowStyle Hidden
    if ($process.ExitCode -ne 0) { throw "Installer exited with code $($process.ExitCode)" }
    if (-not (Test-Path -LiteralPath $AppExe -PathType Leaf)) { throw "Updated DBX executable is missing: $AppExe" }
    Log "Restarting $AppExe"
    Start-Process -FilePath $AppExe -WorkingDirectory (Split-Path -Parent $AppExe)
} catch {
    Log "Update failed: $($_.Exception.Message)"
    if (Test-Path -LiteralPath $AppExe -PathType Leaf) {
        try {
            Start-Process -FilePath $AppExe -WorkingDirectory (Split-Path -Parent $AppExe)
            Log 'Restarted DBX after update failure'
        } catch {
            Log "Failed to restart DBX: $($_.Exception.Message)"
        }
    }
    exit 1
}
