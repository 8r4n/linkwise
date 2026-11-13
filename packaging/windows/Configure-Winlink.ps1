# Launch through the installer or Start menu with Windows PowerShell -STA.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$station = Join-Path $env:ProgramData 'Linkwise\station.json'
$modem = Join-Path $PSScriptRoot 'linkwise-modemd.exe'
$serviceName = 'LinkwiseModem'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Start-Process (Join-Path $PSHOME 'powershell.exe') -Verb RunAs -ArgumentList (
        '-NoProfile -ExecutionPolicy RemoteSigned -STA -File "{0}"' -f $PSCommandPath)
    return
}

$form = [Windows.Forms.Form]::new()
$form.Text = 'Configure Linkwise for Winlink Express'
$form.ClientSize = [Drawing.Size]::new(720, 475)
$form.StartPosition = 'CenterScreen'
$form.FormBorderStyle = 'FixedDialog'
$form.MaximizeBox = $false
$fields = [Windows.Forms.TableLayoutPanel]::new()
$fields.Dock = 'Fill'
$fields.Padding = [Windows.Forms.Padding]::new(15)
$fields.ColumnCount = 2
$fields.AutoScroll = $true
$fields.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Absolute', 175)) | Out-Null
$fields.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 100)) | Out-Null
$form.Controls.Add($fields)

function Add-Field($label, $control) {
    $caption = [Windows.Forms.Label]::new()
    $caption.Text = $label
    $caption.AutoSize = $true
    $caption.Anchor = 'Left'
    $control.Dock = 'Top'
    $fields.Controls.Add($caption)
    $fields.Controls.Add($control)
}
$call = [Windows.Forms.TextBox]::new()
$call.CharacterCasing = 'Upper'
Add-Field 'Station callsign' $call
$port = [Windows.Forms.NumericUpDown]::new()
$port.Minimum = 1024
$port.Maximum = 65533
$port.Value = 8300
Add-Field 'Command port' $port
$mode = [Windows.Forms.ComboBox]::new()
$mode.DropDownStyle = 'DropDownList'
$mode.Items.AddRange(@('Native audio - 500 Hz', 'Native audio - 2300 Hz', 'Native audio - 2750 Hz', 'Radio-free waveform test'))
$mode.SelectedIndex = 0
Add-Field 'Station mode' $mode
$inputDevice = [Windows.Forms.TextBox]::new()
$outputDevice = [Windows.Forms.TextBox]::new()
Add-Field 'Input device name' $inputDevice
Add-Field 'Output device name' $outputDevice
$inputChannel = [Windows.Forms.NumericUpDown]::new()
$outputChannel = [Windows.Forms.NumericUpDown]::new()
$inputChannel.Maximum = 65535
$outputChannel.Maximum = 65535
Add-Field 'Input channel (zero-based)' $inputChannel
Add-Field 'Output channel (zero-based)' $outputChannel
$bandwidths = @(500, 2300, 2750, 500)
$profile = [Windows.Forms.TextBox]::new()
Add-Field 'BW500 measurement profile' $profile
$browse = [Windows.Forms.Button]::new()
$browse.Text = 'Browse for profile'
$browse.Add_Click({
    $dialog = [Windows.Forms.OpenFileDialog]::new()
    $dialog.Filter = 'JSON profiles (*.json)|*.json'
    try {
        if ($dialog.ShowDialog($form) -eq 'OK') { $profile.Text = $dialog.FileName }
    } finally { $dialog.Dispose() }
})
Add-Field '' $browse
$endpoints = [Windows.Forms.Label]::new()
$endpoints.AutoSize = $true
Add-Field 'Winlink Express settings' $endpoints
$update = {
    $command = [int]$port.Value
    $virtual = $mode.SelectedIndex -eq 3
    $bandwidth = $bandwidths[$mode.SelectedIndex]
    $profile.Enabled = $browse.Enabled = $bandwidth -eq 500
    $inputDevice.Enabled = $outputDevice.Enabled = $inputChannel.Enabled = $outputChannel.Enabled = !$virtual
    $endpoints.Text = "127.0.0.1   Command $command   DATA $($command + 1)`r`n$bandwidth Hz   VARA auto-launch off"
    if ($virtual) { $endpoints.Text += "   Waveform peer $($command + 2)" }
}
$port.Add_ValueChanged($update)
$mode.Add_SelectedIndexChanged($update)
& $update
$hint = [Windows.Forms.Label]::new()
$hint.AutoSize = $true
$hint.Text = "Copy device names from linkwise-modemd --audio-devices.`r`nThe waveform test requires a peer before you open Express."
Add-Field '' $hint
$apply = [Windows.Forms.Button]::new()
$apply.Text = 'Apply and start service'
Add-Field '' $apply
$guide = [Windows.Forms.Button]::new()
$guide.Text = 'Open Winlink quick start'
$guide.Add_Click({ Start-Process notepad.exe ('"{0}"' -f (Join-Path $PSScriptRoot 'docs\Winlink-Quick-Start.md')) })
Add-Field '' $guide
$logs = [Windows.Forms.Button]::new()
$logs.Text = 'Open Application event log'
$logs.Add_Click({ Start-Process eventvwr.msc })
Add-Field '' $logs

$existing = if (Test-Path -LiteralPath $station) {
    Get-Content -Raw -LiteralPath $station | ConvertFrom-Json
}
if ($existing) {
    $call.Text = $existing.callsign
    if ($existing.command_port -ge $port.Minimum -and $existing.command_port -le $port.Maximum) {
        $port.Value = $existing.command_port
    }
    if ($existing.winlink_test_audio_port) {
        $mode.SelectedIndex = 3
    } else {
        $inputDevice.Text = $existing.input.name
        $outputDevice.Text = $existing.output.name
        $inputChannel.Value = $existing.input.channel
        $outputChannel.Value = $existing.output.channel
        $index = [array]::IndexOf($bandwidths, [int]$existing.bandwidth)
        if ($index -ge 0) { $mode.SelectedIndex = $index }
    }
    if ($existing.bw500_profile) {
        $profile.Text = [IO.Path]::Combine((Split-Path $station), $existing.bw500_profile)
    }
}

$apply.Add_Click({
    $candidate = Join-Path (Split-Path $station) ("station-{0}.json" -f [guid]::NewGuid())
    $backup = "$candidate.backup"
    $profileCopy = $null
    $wasRunning = $false
    $stopped = $false
    $saved = $false
    try {
        $command = [int]$port.Value
        $virtual = $mode.SelectedIndex -eq 3
        $bandwidth = $bandwidths[$mode.SelectedIndex]
        $lastPort = $command + 1
        $settings = @{
            version = 1
            callsign = $call.Text.Trim()
            command_port = $command
            data_port = $command + 1
        }
        if ($virtual) {
            $lastPort = $command + 2
            $settings.winlink_test_audio_port = $lastPort
            $settings.input = @{name = 'Waveform socket'; channel = 0}
            $settings.output = @{name = 'Waveform socket'; channel = 0}
        } else {
            $settings.bandwidth = $bandwidth
            $settings.input = @{name = $inputDevice.Text.Trim(); channel = [int]$inputChannel.Value}
            $settings.output = @{name = $outputDevice.Text.Trim(); channel = [int]$outputChannel.Value}
        }
        if ($bandwidth -eq 500) {
            if (!$profile.Text.Trim()) { throw 'Select a BW500 measurement profile.' }
            $source = Get-Item -LiteralPath $profile.Text
            if ($source.PSIsContainer -or $source.Length -gt 131072) { throw 'Select a BW500 JSON profile of at most 128 KiB.' }
            $profileCopy = Join-Path (Split-Path $station) ("bw500-{0}.json" -f [guid]::NewGuid())
            Copy-Item -LiteralPath $source.FullName -Destination $profileCopy
            $settings.bw500_profile = Split-Path -Leaf $profileCopy
        }
        [IO.File]::WriteAllText($candidate, ($settings | ConvertTo-Json -Depth 5))
        $validation = & $modem --check-config $candidate 2>&1
        if ($LASTEXITCODE) { throw ($validation -join "`n") }

        $wasRunning = (Get-Service $serviceName).Status -eq 'Running'
        if ($wasRunning) { Stop-Service $serviceName; $stopped = $true }
        $listeners = [Collections.Generic.List[Net.Sockets.TcpListener]]::new()
        try {
            foreach ($number in $command..$lastPort) {
                $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $number)
                $listener.ExclusiveAddressUse = $true
                try { $listener.Start() } catch { throw "Port $number is in use or unavailable. Try command port 18310." }
                $listeners.Add($listener)
            }
        } finally {
            foreach ($listener in $listeners) { $listener.Stop() }
        }
        if (Test-Path -LiteralPath $station) {
            [IO.File]::Replace($candidate, $station, $backup)
        } else { [IO.File]::Move($candidate, $station) }
        $saved = $true
        Start-Service $serviceName
        if ((Get-Service $serviceName).Status -ne 'Running') { throw 'Linkwise did not stay running. Check the Application event log.' }
        [Windows.Forms.MessageBox]::Show($form, "Linkwise service running.`r`n`r`n$($endpoints.Text)", 'Winlink setup') | Out-Null
        $form.Close()
    } catch {
        $message = $_.Exception.Message
        try {
            if ($saved) {
                Stop-Service $serviceName -ErrorAction SilentlyContinue
                if (Test-Path -LiteralPath $backup) { [IO.File]::Replace($backup, $station, [NullString]::Value) }
                else { Remove-Item -LiteralPath $station }
            }
            if ($profileCopy) { Remove-Item -LiteralPath $profileCopy -ErrorAction SilentlyContinue }
            if ($stopped) { Start-Service $serviceName }
        } catch { $message += "`r`nRestore failed: $($_.Exception.Message)" }
        [Windows.Forms.MessageBox]::Show($form, $message, 'Winlink setup') | Out-Null
    } finally {
        Remove-Item -LiteralPath $candidate -ErrorAction SilentlyContinue
    }
})
[void]$form.ShowDialog()
$form.Dispose()
