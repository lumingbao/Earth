Unicode True

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !error "VERSION must be supplied from Cargo metadata"
!endif
!ifndef VI_VERSION
  !error "VI_VERSION must be supplied from Cargo metadata"
!endif
!ifndef VERSION_CHECK_SCRIPT
  !error "VERSION_CHECK_SCRIPT must point to the shared SemVer checker"
!endif
!ifndef EXE_PATH
  !define EXE_PATH "..\..\dist\Earth.exe"
!endif
!ifndef ICON_PATH
  !define ICON_PATH "earth.ico"
!endif
!ifndef OUT_FILE
  !define OUT_FILE "..\..\dist\Earth-${VERSION}-Windows-x64-Setup.exe"
!endif

!define APP_NAME "Earth"
!define APP_PUBLISHER "Earth"
!define APP_EXE "Earth.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"

Name "${APP_NAME} ${VERSION}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma

VIProductVersion "${VI_VERSION}"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "CompanyName" "${APP_PUBLISHER}"
VIAddVersionKey "FileDescription" "${APP_NAME} Clipboard Manager Setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

!define MUI_ICON "${ICON_PATH}"
!define MUI_UNICON "${ICON_PATH}"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "立即启动 ${APP_NAME}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "/oname=earth-semver.js" "${VERSION_CHECK_SCRIPT}"
  Call CheckInstalledVersion
FunctionEnd

Function CheckInstalledVersion
  ; The script reads the fixed registration key itself. Registry text is never
  ; interpolated into a command line, and only Cargo's validated version is passed.
  nsExec::ExecToStack /TIMEOUT=15000 '"$SYSDIR\cscript.exe" //B //NoLogo "$PLUGINSDIR\earth-semver.js" "${VERSION}"'
  Pop $0
  Pop $1
  ${If} $0 == 2
    MessageBox MB_OK|MB_ICONSTOP "A newer Earth version is already installed, including prerelease precedence. Downgrade cancelled."
    Abort
  ${ElseIf} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "Earth could not safely verify the installed version. Installation cancelled; check Windows Script Host availability and the existing installation."
    Abort
  ${EndIf}
FunctionEnd

Section "${APP_NAME} (必选)" SecMain
  SectionIn RO

  ; Recheck after directory selection and before terminating or replacing Earth.
  ; An unregistered existing executable has no reliable full SemVer metadata.
  IfFileExists "$INSTDIR\${APP_EXE}" 0 version_check
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  StrCmp $0 "$INSTDIR" 0 unregistered_install
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "DisplayVersion"
  StrCmp $0 "" unregistered_install version_check
  unregistered_install:
    MessageBox MB_OK|MB_ICONSTOP "This directory contains an Earth installation whose version cannot be verified. Choose an empty directory or explicitly remove the existing installation first."
    Abort
  version_check:
  Call CheckInstalledVersion

  ; Gracefully close any running Earth instance before replacing the binary
  nsExec::ExecToStack 'taskkill /F /IM ${APP_EXE}'
  Pop $0
  Pop $1
  Sleep 250

  SetOutPath "$INSTDIR"
  File "/oname=${APP_EXE}" "${EXE_PATH}"
  File "/oname=earth.ico" "${ICON_PATH}"

  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; Start Menu shortcuts
  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\earth.ico" 0
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\卸载 ${APP_NAME}.lnk" "$INSTDIR\Uninstall.exe"

  ; Desktop shortcut
  CreateShortcut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\earth.ico" 0

  ; Register Add/Remove Programs metadata
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "VersionQuad" "${VI_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${APP_PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "Uninstall"
  nsExec::ExecToStack 'taskkill /F /IM ${APP_EXE}'
  Pop $0
  Pop $1
  Sleep 250

  Delete "$DESKTOP\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\卸载 ${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"

  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\earth.ico"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
