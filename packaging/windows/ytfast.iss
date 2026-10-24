; Inno Setup script. Build with:  iscc /DAppVersion=0.1.0 packaging\windows\ytfast.iss
; Expects the staged folder from packaging/stage.sh at dist\YTFast.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6D1E4C52-8A0B-4E8F-9C41-2B7A5F3D90E1}
AppName=YTFast
AppVersion={#AppVersion}
AppPublisher=AminPerez
DefaultDirName={autopf}\YTFast
DefaultGroupName=YTFast
UninstallDisplayName=YTFast {#AppVersion}
UninstallDisplayIcon={app}\ytfast.exe
SetupIconFile=..\icon.ico
OutputDir=..\..\dist
OutputBaseFilename=YTFast-Setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
; Per-user install by default; the dialog lets users pick.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
DisableProgramGroupPage=yes
LicenseFile=..\..\dist\YTFast\NOTICE.txt

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"

[Files]
Source: "..\..\dist\YTFast\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\YTFast"; Filename: "{app}\ytfast.exe"
Name: "{autodesktop}\YTFast"; Filename: "{app}\ytfast.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\ytfast.exe"; Description: "Launch YTFast"; Flags: nowait postinstall skipifsilent

[Code]
// Offer to delete the app data (saved library, settings, logs, downloaded audio cache).
// Skipped for silent uninstalls and for updates.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
    if MsgBox('Do you also want to delete all YTFast application data (saved library, settings, logs and cached audio)?', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
    begin
      DelTree(ExpandConstant('{userappdata}\YTFast'), True, True, True);
      DelTree(ExpandConstant('{localappdata}\YTFast'), True, True, True);
    end;
end;
