Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "Sections.nsh"
!include "x64.nsh"
!ifndef VERSION
  !error "VERSION is required"
!endif
!ifndef PAYLOAD
  !error "PAYLOAD is required"
!endif
!ifndef OUTPUT
  !error "OUTPUT is required"
!endif
Name "Seek ${VERSION}"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\Seek"
RequestExecutionLevel user
SetCompressor /SOLID zlib
ShowInstDetails show
ShowUninstDetails show
VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "Seek Setup"
VIAddVersionKey /LANG=1033 "FileDescription" "Seek per-user installer"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "Copyright (c) 2026 Seek contributors"
!define MUI_ICON "${PAYLOAD}\seek.ico"
!define MUI_UNICON "${PAYLOAD}\seek.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_NOAUTOCLOSE
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${PAYLOAD}\LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH
!insertmacro MUI_LANGUAGE "English"
Var Fixture
Var RegistryId
Var ExtraArgs
Var DeleteData

Function .onInit
  SetShellVarContext current
  SetRegView 64
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "Seek requires 64-bit Windows."
    Quit
  ${EndIf}
  StrCpy $RegistryId "Seek"
  ${GetParameters} $0
  ${GetOptions} $0 "/FIXTURE=" $Fixture
  StrCmp $Fixture "" done
  ; Install.ps1 validates this marked, TEMP-only fixture before any install write.
  StrCpy $INSTDIR "$Fixture\Program"
  ${GetFileName} $Fixture $1
  StrCpy $1 $1 "" 15
  StrCpy $RegistryId "Seek.PackagingTest.$1"
  done:
FunctionEnd

Section "Seek (required)" Main
  SectionIn RO
  StrCpy $ExtraArgs "-NoDesktopShortcut"
  SectionGetFlags 1 $0
  IntOp $0 $0 & ${SF_SELECTED}
  StrCmp $0 0 +2
  StrCpy $ExtraArgs "-DesktopShortcut"
  StrCmp $Fixture "" +2
  StrCpy $ExtraArgs '$ExtraArgs -TestRoot "$Fixture" -NonInteractive'
  IfFileExists "$INSTDIR\*.*" 0 install
  IfSilent reject 0
  MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Replace the existing Seek program files? Your saved index, settings and document text will be kept. Close Seek before continuing." IDYES install
  reject:
  SetErrorLevel 1
  Abort "Installation cancelled. Existing files were not replaced."
  install:
  InitPluginsDir
  SetOutPath "$PLUGINSDIR\payload"
  File "${PAYLOAD}\*.*"
  DetailPrint "Installing for the current user. No helper service is installed."
  nsExec::ExecToStack '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "$PLUGINSDIR\payload\Install.ps1" -Replace $ExtraArgs'
  Pop $0
  Pop $1
  StrCmp $0 "0" success
  DetailPrint "$1"
  IfSilent +2 0
  MessageBox MB_OK|MB_ICONSTOP "Seek could not be installed. Close Seek and try again.$\r$\n$1"
  SetErrorLevel 1
  Abort "Install failed."
  success:
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  IfErrors native_failed
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\$RegistryId" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\$RegistryId" "QuietUninstallString" '$\"$INSTDIR\Uninstall.exe$\" /S'
  Goto finished
  native_failed:
  SetErrorLevel 1
  Abort "Program installed, but the native uninstaller could not be written. Uninstall.cmd remains available."
  finished:
SectionEnd
Section /o "Desktop shortcut" DesktopShortcut
  ; Optional choice is consumed by the required section before files are copied.
SectionEnd

Function un.onInit
  SetShellVarContext current
  SetRegView 64
  StrCpy $DeleteData "-KeepData"
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/DELETE_DATA" $1
  IfErrors +2 0
  StrCpy $DeleteData "-DeleteData"
FunctionEnd
Section "Uninstall"
  IfSilent remove 0
  MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Also delete your saved Seek index, settings and document-text cache? Choose No to keep them (recommended)." IDNO remove
  StrCpy $DeleteData "-DeleteData"
  remove:
  nsExec::ExecToStack '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\Uninstall.ps1" -Confirmed $DeleteData -KeepHelper'
  Pop $0
  Pop $1
  StrCmp $0 "0" done
  DetailPrint "$1"
  IfSilent +2 0
  MessageBox MB_OK|MB_ICONSTOP "Seek could not be uninstalled. Close Seek and try again.$\r$\n$1"
  SetErrorLevel 1
  Abort "Uninstall failed."
  done:
  SetOutPath "$TEMP"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DetailPrint "Program removed. Any opt-in helper service is retained; see README for removal instructions."
SectionEnd
