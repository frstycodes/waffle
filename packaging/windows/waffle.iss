; Built by .github/workflows/release.yml: iscc /DVersion=<x.y.z> /DExe=<path to waffle.exe>
#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
AppId={{62BFCB0C-BB1C-47AE-A8E4-C16EEE4043D0}
AppName=Waffle
AppVersion={#Version}
AppPublisher=frstycodes
; Per-user, no admin prompt: the updater replaces Waffle.exe in place, so its folder must be writable
; by the user (Program Files is not).
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\Waffle
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
OutputBaseFilename=waffle-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
SetupIconFile=..\..\assets\windows\app.ico
UninstallDisplayIcon={app}\Waffle.exe
CloseApplications=yes

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "Waffle.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Waffle"; Filename: "{app}\Waffle.exe"
Name: "{autodesktop}\Waffle"; Filename: "{app}\Waffle.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Waffle.exe"; Description: "Launch Waffle"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; What an update set aside while Waffle was running.
Type: files; Name: "{app}\Waffle.exe.old"
; The picked app icon, written for the shortcuts (src/app_icon/windows.rs).
Type: files; Name: "{app}\Waffle-icon.ico"
