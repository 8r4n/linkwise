# Run as administrator on a disposable machine with the Linkwise installer applied.
$ErrorActionPreference = 'Stop'
$name = 'LinkwiseModem'
$config = Join-Path $env:ProgramData 'Linkwise\station.json'
if (Test-Path -LiteralPath $config) { throw "Use a clean station without an existing configuration" }

function Get-StationStatus {
    $client = [Net.Sockets.TcpClient]::new()
    try {
        $client.Connect('127.0.0.1', 8300)
        $stream = $client.GetStream()
        $stream.ReadTimeout = 1000
        $reader = [IO.StreamReader]::new($stream)
        return $reader.ReadLine() | ConvertFrom-Json
    } finally {
        $client.Dispose()
    }
}

function Wait-Station([int] $previousPid = 0) {
    $deadline = [DateTime]::UtcNow.AddSeconds(25)
    do {
        $service = Get-CimInstance Win32_Service -Filter "Name='$name'"
        if ($service.State -eq 'Running' -and $service.ProcessId -ne $previousPid) {
            try {
                $status = Get-StationStatus
                if ($status.ready -eq $false -and $status.received_frames -eq 0) {
                    return $service
                }
            } catch [Net.Sockets.SocketException] {
                # SCM can observe the restart before this test can connect.
            }
        }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)
    throw 'The station did not expose degraded status within 25 seconds'
}

try {
    @{
        version = 1
        callsign = 'W1AW'
        input = @{ name = "Linkwise CI missing device $([Guid]::NewGuid())"; channel = 0 }
        output = @{ name = 'Unused output'; channel = 0 }
    } | ConvertTo-Json | Set-Content -LiteralPath $config -Encoding ascii

    Start-Service $name
    $service = Wait-Station
    if ($service.StartName -ne 'NT AUTHORITY\LocalService') { throw 'Unexpected service account' }
    if ((Get-Process -Id $service.ProcessId).SessionId -ne 0) { throw 'Expected Session 0' }
    Stop-Process -Id $service.ProcessId -Force
    $null = Wait-Station $service.ProcessId

    Stop-Service $name
    if ((Get-Service $name).Status -ne 'Stopped') { throw 'Service did not stop' }
    $events = Get-WinEvent -FilterHashtable @{
        LogName = 'Application'
        ProviderName = $name
        StartTime = [DateTime]::Now.AddMinutes(-5)
    }
    if (-not ($events | Where-Object { $_.Properties.Value -match 'Degraded' })) {
        throw 'Missing station diagnostic in the Application event log'
    }
} finally {
    Stop-Service $name -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $config -Force -ErrorAction SilentlyContinue
}
