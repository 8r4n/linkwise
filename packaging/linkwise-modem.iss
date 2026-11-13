[Setup]
AppId=io.github.8r4n.linkwise.modem
AppName=Linkwise Modem Preview
AppPublisher=Linkwise project
AppPublisherURL=https://github.com/8r4n/linkwise
AppVersion={#AppVersion}
ArchitecturesAllowed={#AppArchitecture}
ArchitecturesInstallIn64BitMode={#AppArchitecture}
MinVersion=10.0.22000
DefaultDirName={autopf}\Linkwise Modem
DefaultGroupName=Linkwise Modem
PrivilegesRequired=admin
UsePreviousAppDir=no
DisableDirPage=yes
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

Source: "{#StageDir}\Configure-Winlink.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\Winlink-Quick-Start.md"; DestDir: "{app}\docs"; Flags: ignoreversion

[Icons]
Name: "{group}\Configure Linkwise for Winlink"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy RemoteSigned -STA -File ""{app}\Configure-Winlink.ps1"""
Name: "{group}\Winlink Quick Start"; Filename: "{sys}\notepad.exe"; Parameters: """{app}\docs\Winlink-Quick-Start.md"""

[Run]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy RemoteSigned -STA -File ""{app}\Configure-Winlink.ps1"""; Description: "Configure Linkwise for Winlink"; Flags: postinstall nowait skipifsilent

[Icons]
Name: "{group}\Linkwise Modem PowerShell"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoLogo"; WorkingDir: "{app}"
Name: "{group}\Installation Guide"; Filename: "{sys}\notepad.exe"; Parameters: """{app}\docs\Install.md"""
Name: "{group}\Station Setup"; Filename: "{sys}\notepad.exe"; Parameters: """{app}\docs\Station-setup.md"""

[Registry]
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Services\EventLog\Application\LinkwiseModem"; ValueType: expandsz; ValueName: "EventMessageFile"; ValueData: "{sys}\EventCreate.exe"; Flags: uninsdeletekey
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Services\EventLog\Application\LinkwiseModem"; ValueType: dword; ValueName: "TypesSupported"; ValueData: "7"

[Code]
const
  ServiceName = 'LinkwiseModem';
  ServiceDescription = 'Provides the Linkwise software modem for Winlink Express and other compatible applications.';

var
  RestartStation: Boolean;

function ServiceCommand(Arguments: String; var ResultCode: Integer): Boolean;
begin
  Result := Exec(ExpandConstant('{sys}\sc.exe'), Arguments, '',
    SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function ServiceExists(): Boolean;
var
  Code: Integer;
begin
  Result := ServiceCommand('query ' + ServiceName, Code) and (Code = 0);
end;

function RunPowerShell(Command: String): Boolean;
var
  Code: Integer;
begin
  Result := Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -Command "' + Command + '"',
    '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0);
end;

function StopStation(): Boolean;
begin
  Result := not ServiceExists() or RunPowerShell(
    'try { $s = Get-Service LinkwiseModem -ErrorAction Stop; ' +
    'if ($s.Status -ne 1 -and $s.Status -ne 3) { $s.Stop() }; ' +
    '$s.WaitForStatus(1, [TimeSpan]::FromSeconds(20)) } catch { exit 1 }');
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  RestartStation := ServiceExists() and RunPowerShell(
    'if ((Get-Service LinkwiseModem).Status -ne 4) { exit 1 }');
  if not StopStation() then
    Result := 'Could not stop the Linkwise modem service.';
end;

function SecureConfigDirectory(): Boolean;
begin
  Result := RunPowerShell(
    '$ErrorActionPreference = ''Stop''; try { ' +
    '$p = Join-Path $env:ProgramData ''Linkwise''; ' +
    '$acl = [System.Security.AccessControl.DirectorySecurity]::new(); ' +
    '$acl.SetSecurityDescriptorSddlForm(''O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;GRGX;;;LS)''); ' +
    'if (Test-Path -LiteralPath $p) { ' +
    '$d = Get-Item -LiteralPath $p -Force; if (!$d.PSIsContainer -or ' +
    '($d.Attributes -band [IO.FileAttributes]::ReparsePoint)) { exit 1 }; ' +
    '$old = Get-Acl -LiteralPath $p; ' +
    'if ($old.GetOwner([System.Security.Principal.SecurityIdentifier]).Value -notin ' +
    '@(''S-1-5-18'', ''S-1-5-32-544'')) { exit 1 }; Set-Acl -LiteralPath $p -AclObject $acl ' +
    '} else { [IO.Directory]::CreateDirectory($p, $acl) | Out-Null } ' +
    '} catch { exit 1 }');
end;

procedure CurStepChanged(Step: TSetupStep);
var
  Code: Integer;
  ConfigPath, CommandLine, Arguments: String;
begin
  if Step <> ssPostInstall then Exit;
  if not SecureConfigDirectory() then
    RaiseException('Could not secure the Linkwise station configuration directory.');
  ConfigPath := ExpandConstant('{commonappdata}\Linkwise\station.json');
  CommandLine := '\"' + ExpandConstant('{app}\linkwise-modemd.exe') +
    '\" --windows-service \"' + ConfigPath + '\"';
  if ServiceExists() then
    Arguments := 'config ' + ServiceName + ' binPath= "' + CommandLine + '"'
  else
    Arguments := 'create ' + ServiceName + ' binPath= "' + CommandLine +
      '" start= demand obj= "NT AUTHORITY\LocalService" DisplayName= "Linkwise Modem"';
  if not ServiceCommand(Arguments, Code) or (Code <> 0) then
    RaiseException('Could not register the Linkwise modem service.');
  if not ServiceCommand('description ' + ServiceName + ' "' + ServiceDescription + '"', Code) or (Code <> 0) then
    RaiseException('Could not set the Linkwise service description.');
  if not ServiceCommand('sidtype ' + ServiceName + ' unrestricted', Code) or (Code <> 0) then
    RaiseException('Could not configure the Linkwise service SID.');
  if not ServiceCommand('failure ' + ServiceName +
    ' reset= 86400 actions= restart/5000/restart/30000/none/0', Code) or (Code <> 0) then
    RaiseException('Could not configure Linkwise service recovery.');
  if RestartStation and (not ServiceCommand('start ' + ServiceName, Code) or (Code <> 0)) then
    RaiseException('The upgraded Linkwise service could not be restarted. See the Application event log.');
end;

function InitializeUninstall(): Boolean;
var
  Code: Integer;
begin
  Result := StopStation();
  if Result and ServiceExists() then
    Result := ServiceCommand('delete ' + ServiceName, Code) and (Code = 0);
end;
