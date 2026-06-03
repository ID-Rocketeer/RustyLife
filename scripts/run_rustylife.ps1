$ProjectRoot = Split-Path $PSScriptRoot -Parent
$Executable = "$ProjectRoot\target\release\rustylife-server.exe"
$Arguments = @("--stay-awake", "--log", "--log-interval", "5")
$LogPath = "$ProjectRoot\logs\server_console.log"

# Create logs directory if it doesn't exist
$LogDir = Split-Path $LogPath
if (-not (Test-Path $LogDir)) {
    New-Item -ItemType Directory -Path $LogDir -Force
}

Write-Output "Starting RustyLife monitor loop..."

while ($true) {
    $Timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    "[$Timestamp] Starting RustyLife Server..." | Out-File -FilePath $LogPath -Append -Encoding utf8
    
    # Run the server and wait for it to exit/crash
    # Stdout and Stderr are redirected to the log file together
    & $Executable $Arguments *>> $LogPath
    
    $ExitCode = $LASTEXITCODE
    $Timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    "[$Timestamp] RustyLife Server stopped with exit code $ExitCode. Restarting in 2 seconds..." | Out-File -FilePath $LogPath -Append -Encoding utf8
    
    Start-Sleep -Seconds 2
}
