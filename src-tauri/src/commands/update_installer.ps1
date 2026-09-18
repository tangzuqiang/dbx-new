param(
    [Parameter(Mandatory=$true)][int]$OldPid,
    [Parameter(Mandatory=$true)][string]$Installer,
    [Parameter(Mandatory=$true)][string]$AppExe,
    [string]$SilentArgs = '/S /UPDATE /R'
)

$ErrorActionPreference = 'Stop'
$log = Join-Path (Split-Path -Parent $Installer) 'install-update.log'
function Log([string]$Message) {
    Add-Content -LiteralPath $log -Value "$(Get-Date -Format o) $Message"
}

try {
    Log "Waiting for DBX process $OldPid to exit"
    $deadline = (Get-Date).AddSeconds(90)
    while (Get-Process -Id $OldPid -ErrorAction SilentlyContinue) {
        if ((Get-Date) -gt $deadline) { throw 'Timed out waiting for DBX to exit' }
        Start-Sleep -Milliseconds 300
    }
    if (-not (Test-Path -LiteralPath $Installer -PathType Leaf)) { throw 'Installer is missing' }
    Log "Installing $Installer"
    $allowedArgs = @('/S', '/UPDATE', '/R')
    $installArgs = @($SilentArgs -split '\s+' | Where-Object { $_ })
    if ($installArgs.Count -eq 0 -or @($installArgs | Where-Object { $_ -notin $allowedArgs }).Count -gt 0) { throw 'Invalid installer arguments in update manifest' }
    $process = Start-Process -FilePath $Installer -ArgumentList $installArgs -PassThru -Wait -WindowStyle Hidden
    if ($process.ExitCode -ne 0) { throw "Installer exited with code $($process.ExitCode)" }
    if (-not (Test-Path -LiteralPath $AppExe -PathType Leaf)) { throw "Updated DBX executable is missing: $AppExe" }
    if (Get-Process -Name 'DBX' -ErrorAction SilentlyContinue) {
        Log 'DBX was restarted by the installer'
        exit 0
    }
    Log "Restarting $AppExe"
    Start-Process -FilePath $AppExe
} catch {
    Log "Update failed: $($_.Exception.Message)"
    exit 1
}
