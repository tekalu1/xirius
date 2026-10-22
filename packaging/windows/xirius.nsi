; The Windows installer of Xirius (LIC-019): NSIS 3, for the current user only.
;
; It needs no administrator rights: the programs go to %LOCALAPPDATA%\Programs\Xirius (or the
; folder chosen on the page, or /D=<folder>), the shortcut to the user's Start menu and the
; uninstall entry to HKCU. The songs and settings (%LOCALAPPDATA%\Xirius, %APPDATA%\Xirius) are
; not the installer's and stay when it uninstalls.
;
; /S installs without a window. A running Xirius is waited for (30 s when silent) and the
; install stops when it does not close, so an update can start this and quit itself.
; /XRUN (with /S) starts Xirius again when the install ends (UI-282: 「更新して再起動」).
;
; Made by scripts/release.py, which stages the files and writes the two lists it includes:
;   makensis -INPUTCHARSET UTF8 -DVERSION=1.0.0 -DVERSION4=1.0.0.0 -DSTAGE=<folder>
;            -DLISTS=<folder> -DOUTFILE=<setup.exe> packaging/windows/xirius.nsi
; STAGE holds what is installed; LISTS holds install-files.nsh and uninstall-files.nsh.
; The file is UTF-8 (the Japanese strings); -INPUTCHARSET UTF8 says so to makensis.

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!macro NEED NAME
  !ifndef ${NAME}
    !error "define ${NAME} (see the top of this file)"
  !endif
!macroend
!insertmacro NEED VERSION
!insertmacro NEED VERSION4
!insertmacro NEED STAGE
!insertmacro NEED LISTS
!insertmacro NEED OUTFILE

!define APP "Xirius"
!define PUBLISHER "The Xirius Authors"
!define URL "https://github.com/tekalu1/xirius"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Xirius"

!include MUI2.nsh
!include LogicLib.nsh
!include FileFunc.nsh

Name "${APP}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
BrandingText "${APP} ${VERSION}"

!define MUI_ICON "${__FILEDIR__}\..\..\assets\brand\xirius.ico"
!define MUI_UNICON "${__FILEDIR__}\..\..\assets\brand\xirius.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\xirius-ui.exe"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; The first is used when Windows is in neither language.
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Japanese"

LangString Running ${LANG_ENGLISH} "Xirius is running ($0). Close it, then click Retry."
LangString Running ${LANG_JAPANESE} "Xirius が動いています（$0）。閉じてから「再試行」を押してください。"

; /XRUN: start Xirius again after a silent install (the app's 「更新して再起動」, UI-282).
Var XRun

Function .onInit
  StrCpy $XRun 0
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/XRUN" $R1
  ${IfNot} ${Errors}
    StrCpy $XRun 1
  ${EndIf}
FunctionEnd

VIProductVersion "${VERSION4}"
VIFileVersion "${VERSION4}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "ProductName" "${APP}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "FileDescription" "${APP} ${VERSION} setup"
VIAddVersionKey /LANG=${LANG_ENGLISH} "CompanyName" "${PUBLISHER}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "LegalCopyright" "Copyright (c) ${PUBLISHER}. MIT OR Apache-2.0"

; $0 = the first program of Xirius in $INSTDIR that is running, or "". A running program cannot
; be opened for writing.
!macro IN_USE NAME
  ${If} $0 == ""
  ${AndIf} ${FileExists} "$INSTDIR\${NAME}"
    ClearErrors
    FileOpen $1 "$INSTDIR\${NAME}" a
    ${If} ${Errors}
      StrCpy $0 "${NAME}"
    ${Else}
      FileClose $1
    ${EndIf}
  ${EndIf}
!macroend

; Waits until no program of Xirius in $INSTDIR runs; stops the install when it is not closed.
!macro WAIT_CLOSED UN
Function ${UN}WaitClosed
  StrCpy $2 0
  ${Do}
    StrCpy $0 ""
    !insertmacro IN_USE "xirius-ui.exe"
    !insertmacro IN_USE "xirius.exe"
    !insertmacro IN_USE "xirius-engine.exe"
    !insertmacro IN_USE "xirius-plugin-host.exe"
    !insertmacro IN_USE "xirius-plugin-scan.exe"
    ${If} $0 == ""
      Return
    ${EndIf}
    ${If} ${Silent}
    ${AndIf} $2 < 60
      Sleep 500
      IntOp $2 $2 + 1
      ${Continue}
    ${EndIf}
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$(Running)" /SD IDCANCEL IDRETRY retry
    ; A silent update cannot ask: put the running Xirius back, and leave a note for the next
    ; start (UI-282).
    ${If} ${Silent}
    ${AndIf} "${UN}" == ""
      Call UpdateFailed
    ${EndIf}
    SetErrorLevel 3
    Abort
    retry:
  ${Loop}
FunctionEnd
!macroend
!insertmacro WAIT_CLOSED ""
!insertmacro WAIT_CLOSED "un."

; /XRUN: put the old Xirius back and leave the note the next start reads (UI-282). A manual
; install writes nothing.
Function UpdateFailed
  ${If} $XRun == 1
    Exec '"$INSTDIR\xirius-ui.exe"'
    CreateDirectory "$LOCALAPPDATA\Xirius\keep"
    FileOpen $3 "$LOCALAPPDATA\Xirius\keep\update-failed.txt" w
    FileClose $3
  ${EndIf}
FunctionEnd

Section "Xirius" Main
  SectionIn RO
  Call WaitClosed
  SetOutPath "$INSTDIR"
  ; A file that cannot be written (a scanner's lock, a full disk) must not leave a half
  ; install with no word: put Xirius back and leave the note (UI-282).
  ClearErrors
  !include "${LISTS}\install-files.nsh"
  Goto files_done
  update_failed:
    Call UpdateFailed
    SetErrorLevel 4
    Abort
  files_done:
  ClearErrors
  WriteUninstaller "$INSTDIR\uninstall.exe"
  ${If} ${Errors}
    Call UpdateFailed
    SetErrorLevel 4
    Abort
  ${EndIf}
  CreateShortcut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\xirius-ui.exe"

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "${URL}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\xirius-ui.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"
  ; UI-282: a silent update asked for Xirius to come back.
  ${If} $XRun == 1
  ${AndIf} ${Silent}
    Exec '"$INSTDIR\xirius-ui.exe"'
  ${EndIf}
SectionEnd

; Removes only what the installer put there, by name; a folder is removed when it is empty.
Section "Uninstall"
  Call un.WaitClosed
  !include "${LISTS}\uninstall-files.nsh"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
