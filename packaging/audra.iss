#define MyAppName "Audra"
#define MyAppVersion GetEnv("VER")
#define MyAppPublisher "Amurpo"
#define MyAppExeName "audra.exe"
#define MyAppId "{{B7C3A1F2-4E9D-4B8A-A012-3F5C6D7E8901}"

[Setup]
AppId={#MyAppId}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
OutputDir=.
OutputBaseFilename=audra-{#MyAppVersion}-windows-x64-setup
SetupIconFile=..\data\icons\audra.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
UninstallDisplayIcon={app}\audra.ico

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\dist\audra\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\audra.ico"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\audra.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#MyAppName}}"; Flags: nowait postinstall skipifsilent

[CustomMessages]
english.AskRemoveData=Do you want to delete your Audra data (music library database, settings and cached cover art)?%n%nChoose No to keep it for a future reinstall.
spanish.AskRemoveData=¿Deseas eliminar tus datos de Audra (base de datos de la biblioteca, ajustes y carátulas en caché)?%n%nElige No para conservarlos para una reinstalación futura.
english.AlreadyInstalledSame=Audra %1 is already installed. Setup will repair / reinstall this version.
spanish.AlreadyInstalledSame=Audra %1 ya está instalado. El asistente reparará / reinstalará esta versión.
english.AlreadyInstalledOther=Audra %1 is already installed. Setup will update it to version %2.
spanish.AlreadyInstalledOther=Audra %1 ya está instalado. El asistente lo actualizará a la versión %2.

[Code]
{ Uninstall registry subkey Inno creates for this AppId (GUID matches MyAppId). }
const
  UninstReg = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{B7C3A1F2-4E9D-4B8A-A012-3F5C6D7E8901}_is1';

{ Read the version of an existing install, if any (admin or per-user). }
function GetInstalledVersion(var Version: string): Boolean;
begin
  Result := RegQueryStringValue(HKLM, UninstReg, 'DisplayVersion', Version) or
            RegQueryStringValue(HKCU, UninstReg, 'DisplayVersion', Version);
end;

{ Tell the user whether this run is a repair/reinstall or an update. }
function InitializeSetup(): Boolean;
var
  Prev: string;
begin
  Result := True;
  if GetInstalledVersion(Prev) then
  begin
    if Prev = '{#MyAppVersion}' then
      MsgBox(FmtMessage(CustomMessage('AlreadyInstalledSame'), [Prev]),
             mbInformation, MB_OK)
    else
      MsgBox(FmtMessage(CustomMessage('AlreadyInstalledOther'),
             [Prev, '{#MyAppVersion}']), mbInformation, MB_OK);
  end;
end;

{ On uninstall, offer to keep or delete user data (db, settings, covers). }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  AppData, LocalData: string;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    AppData := ExpandConstant('{userappdata}\audra');
    LocalData := ExpandConstant('{localappdata}\audra');
    if DirExists(AppData) or DirExists(LocalData) then
    begin
      if MsgBox(CustomMessage('AskRemoveData'),
                mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      begin
        DelTree(AppData, True, True, True);
        DelTree(LocalData, True, True, True);
      end;
    end;
  end;
end;
