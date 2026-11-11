[Setup]
AppId=io.github.8r4n.linkwise.modem
AppName=Linkwise Modem Preview
AppPublisher=Linkwise project
AppPublisherURL=https://github.com/8r4n/linkwise
AppVersion={#AppVersion}
ArchitecturesAllowed={#AppArchitecture}
ArchitecturesInstallIn64BitMode={#AppArchitecture}
MinVersion=10.0.22000
DefaultDirName={localappdata}\Programs\Linkwise Modem
DefaultGroupName=Linkwise Modem
PrivilegesRequired=lowest
UninstallDisplayName=Linkwise Modem Preview
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
InfoBeforeFile={#StageDir}\Install.md

[Files]
Source: "{#StageDir}\linkwise.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\linkwise-modemd.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\Install.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "{#StageDir}\Station-setup.md"; DestDir: "{app}\docs"; Flags: ignoreversion

[Icons]
Name: "{group}\Linkwise Modem PowerShell"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoLogo"; WorkingDir: "{app}"
Name: "{group}\Installation Guide"; Filename: "{sys}\notepad.exe"; Parameters: """{app}\docs\Install.md"""
Name: "{group}\Station Setup"; Filename: "{sys}\notepad.exe"; Parameters: """{app}\docs\Station-setup.md"""
