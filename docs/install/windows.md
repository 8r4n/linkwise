# Linkwise Modem Preview for Windows

Linkwise provides command-line station checks and modem simulations. Live
Winlink message transfer and radio PTT are still under development.

## Install or update

1. In Settings → System → About, check System type. Choose the
   `windows-arm64.exe` installer for Arm or `windows-x86_64.exe` for x64.
2. Run Setup. It installs for the current user without administrator rights.
   To update, close any Linkwise commands and run the newer installer.
3. Open **Linkwise Modem PowerShell** from Start and run:

   ```powershell
   .\linkwise-modemd.exe --version
   .\linkwise-modemd.exe --simulate
   ```

The simulation runs locally without a radio. Development installers can be
unsigned. If Windows rejects the installer, ask the distributor for a signed
build.

## Set up your station

Open **Station Setup** from Start for device and configuration instructions.
Save `station.json` in your Documents folder and pass its full path to the
station commands. For example:

```powershell
$station = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'station.json'
.\linkwise-modemd.exe --check-station $station
```

If capture is denied, check Settings → Privacy & security → Microphone and
the setting that allows desktop apps to access it.

## Uninstall

Close running Linkwise commands. In Settings → Apps → Installed apps,
uninstall **Linkwise Modem Preview**. Station files saved in Documents
remain available.
