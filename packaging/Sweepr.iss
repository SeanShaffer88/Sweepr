; Sweepr installer (Inno Setup 6). Per-user install: no admin rights needed
; to install the app itself; Sweepr asks for elevation only when a batch of
; system-scope packages is updated.
; Build via packaging\package.ps1, or: ISCC.exe /DVersion=0.2.1 /DExe=<path to sweepr.exe> Sweepr.iss
#ifndef Version
  #define Version "0.2.1"
#endif
#ifndef Exe
  #define Exe "..\target\release\sweepr.exe"
#endif

[Setup]
AppId={{8C4E2A6F-5B1D-4F7A-9E3C-1D2B3A4C5E6F}
AppName=Sweepr
AppVersion={#Version}
AppPublisher=Sean Shaffer
DefaultDirName={localappdata}\Programs\Sweepr
DefaultGroupName=Sweepr
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=Sweepr-v{#Version}-x64-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName=Sweepr {#Version}

[Files]
Source: "{#Exe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\CHANGELOG.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\CREDITS.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Sweepr"; Filename: "{app}\sweepr.exe"
Name: "{autodesktop}\Sweepr"; Filename: "{app}\sweepr.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"

[Run]
Filename: "{app}\sweepr.exe"; Description: "Launch Sweepr"; Flags: nowait postinstall skipifsilent

; Uninstall removes the program only. User data in %LOCALAPPDATA%\Sweepr
; (settings, logs, job journals) is kept; remove it with `sweepr --uninstall`.
