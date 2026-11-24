; Pillbox installer (Inno Setup 6). Build it with installer\build.ps1 after `cargo build --release`.
; Installs for the current user only (no administrator prompt) into %LOCALAPPDATA%\Programs\Pillbox.

#define AppName "Pillbox"
#define AppVersion "0.1.4"

[Setup]
; A fixed id, so a newer installer upgrades an older install instead of installing twice.
AppId={{6F3A1D5E-8C42-4B7A-9E10-5D2B7C94A1F3}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Pillbox
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
; Windows 10 version 1809 or newer, or Windows 11 (screen capture and media controls need 1809).
MinVersion=10.0.17763
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=Pillbox-Setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\pillbox.exe
; The licence and the terms of use, shown before installing; they must be accepted.
; installer\build.ps1 writes this file from LICENSE and TERMS.md.
LicenseFile=EULA.txt
AppCopyright=Copyright (c) 2026 David Sejkora

[Tasks]
Name: "startup"; Description: "Start Pillbox when I sign in to Windows"; Flags: checkedonce
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "..\target\release\pillbox.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\pillboxctl.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\TERMS.md"; DestDir: "{app}"; DestName: "TERMS.txt"; Flags: ignoreversion
Source: "..\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\pillbox.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\pillbox.exe"; Tasks: desktopicon

[Registry]
; The same entry Settings > "Start with Windows" writes, so the two agree. Removed on uninstall.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Pillbox"; ValueData: """{app}\pillbox.exe"""; Flags: uninsdeletevalue; Tasks: startup

[Run]
Filename: "{app}\pillbox.exe"; Description: "Start Pillbox now"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM pillbox.exe"; Flags: runhidden; RunOnceId: "StopPillbox"

[Code]
// A running Pillbox keeps its .exe locked: stop it before files are replaced.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM pillbox.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Result := '';
end;
