; Inno Setup script for the Pustman Windows installer (.github/workflows/beta-release.yml,
; package-windows). Per-user install by default, so no administrator rights are needed;
; the installer offers an all-users install when run elevated.
;
; Build: iscc /DAppVersion=0.1.0-beta.6 /DSourceDir=<dir with pustman.exe> /DOutputBase=<name> pustman.iss

#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\target\release"
#endif
#ifndef OutputBase
  #define OutputBase "pustman-setup"
#endif

[Setup]
; The AppId identifies Pustman to Windows' Apps & features across versions: never change it.
AppId={{F4AD39AB-72B5-445F-BF91-C4F71E532475}
AppName=Pustman
AppVersion={#AppVersion}
AppVerName=Pustman {#AppVersion}
AppPublisher=Pustman
DefaultDirName={autopf}\Pustman
DefaultGroupName=Pustman
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\..\assets\pustman-icon-pack\windows\app-icon.ico
UninstallDisplayIcon={app}\pustman.exe
UninstallDisplayName=Pustman
OutputDir=..\..\out
OutputBaseFilename={#OutputBase}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\pustman.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Pustman"; Filename: "{app}\pustman.exe"
Name: "{autodesktop}\Pustman"; Filename: "{app}\pustman.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\pustman.exe"; Description: "{cm:LaunchProgram,Pustman}"; Flags: nowait postinstall skipifsilent
