; Pillbox installer (Inno Setup 6). Build it with installer\build.ps1 after `cargo build --release`.
; Installs for the current user only (no administrator prompt) into %LOCALAPPDATA%\Programs\Pillbox.

#define AppName "Pillbox"
#define AppVersion "0.1.3"

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
const
  WEBVIEW2_KEY = 'Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';

// The chat drawer needs the Microsoft Edge WebView2 Runtime. Windows 11 always has it; most
// Windows 10 PCs get it with Edge, but not all.
function HasWebView2: Boolean;
var
  Version: String;
begin
  Result :=
    (RegQueryStringValue(HKLM, 'SOFTWARE\WOW6432Node\' + WEBVIEW2_KEY, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0')) or
    (RegQueryStringValue(HKLM, 'SOFTWARE\' + WEBVIEW2_KEY, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0')) or
    (RegQueryStringValue(HKCU, 'Software\' + WEBVIEW2_KEY, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0'));
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssDone) and not WizardSilent and not HasWebView2 then
    MsgBox('Pillbox is installed. One optional part, the chat drawer, needs the free ' +
      'Microsoft Edge WebView2 Runtime, which this PC does not have yet.' + #13#10#13#10 +
      'Everything else works without it. To add it later, search the web for ' +
      '"WebView2 Runtime download" (Microsoft''s page) and install the Evergreen Bootstrapper.',
      mbInformation, MB_OK);
end;

// A running Pillbox keeps its .exe locked: stop it before files are replaced.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM pillbox.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Result := '';
end;
