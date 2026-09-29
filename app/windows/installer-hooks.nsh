; Hooks for Tauri's NSIS installer (bundle.windows.nsis.installerHooks):
; install the command line tool ecm-discovery-cli.exe next to the app and add
; the install directory to the user's PATH; the uninstaller reverts both.
;
; PATH is only changed if it could be read completely: NSIS strings have a
; fixed maximum length (NSIS_MAX_STRLEN), and writing back a truncated PATH
; would destroy the user's PATH. In that case the tool is still installed,
; only not added to PATH.

!include "LogicLib.nsh"
!include "WinMessages.nsh"

; Directory of this file, fixed here: inside a macro ${__FILEDIR__} would be
; the directory of the file the macro is inserted into
!define ECM_HOOKS_DIR "${__FILEDIR__}"

!macro ECM_READ_USER_PATH
  ; $0 = user PATH, $1 = 1 if it can be changed safely
  StrCpy $1 1
  ClearErrors
  ReadRegStr $0 HKCU "Environment" "Path"
  ${If} ${Errors}
    ; No user PATH yet is fine, a PATH that could not be read is not
    StrCpy $0 ""
    StrCpy $2 0
    ${Do}
      EnumRegValue $3 HKCU "Environment" $2
      ${If} $3 == ""
        ${Break}
      ${EndIf}
      ${If} $3 == "Path"
        StrCpy $1 0
        ${Break}
      ${EndIf}
      IntOp $2 $2 + 1
    ${Loop}
  ${Else}
    StrLen $2 $0
    IntOp $3 ${NSIS_MAX_STRLEN} - 1
    ${If} $2 >= $3
      StrCpy $1 0
    ${EndIf}
  ${EndIf}
!macroend

; Tauri's installer code around the hooks may use the registers too
!macro ECM_SAVE_REGISTERS
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  Push $7
!macroend

!macro ECM_RESTORE_REGISTERS
  Pop $7
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

!macro ECM_BROADCAST_ENVIRONMENT
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_POSTINSTALL
  SetOutPath "$INSTDIR"
  ; Built by `npm run build:cli` (beforeBuildCommand) before the bundler runs
  File "${ECM_HOOKS_DIR}\..\..\target\release\ecm-discovery-cli.exe"

  !insertmacro ECM_SAVE_REGISTERS
  !insertmacro ECM_READ_USER_PATH
  ${If} $1 == 1
    ; Already in PATH (e.g. reinstall)? Compare with separators on both sides.
    StrCpy $2 ";$0;"
    StrCpy $3 ";$INSTDIR;"
    StrLen $4 $3
    StrCpy $5 0
    StrCpy $6 0
    ${Do}
      StrCpy $7 $2 $4 $5
      ${If} $7 == ""
        ${Break}
      ${EndIf}
      ${If} $7 == $3
        StrCpy $6 1
        ${Break}
      ${EndIf}
      IntOp $5 $5 + 1
    ${Loop}
    ${If} $6 == 0
      ${If} $0 == ""
        StrCpy $0 "$INSTDIR"
      ${Else}
        StrCpy $0 "$0;$INSTDIR"
      ${EndIf}
      WriteRegExpandStr HKCU "Environment" "Path" $0
      !insertmacro ECM_BROADCAST_ENVIRONMENT
    ${EndIf}
  ${Else}
    DetailPrint "PATH is too long to change safely; ecm-discovery-cli.exe is in $INSTDIR"
  ${EndIf}
  !insertmacro ECM_RESTORE_REGISTERS
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Before Tauri removes the install directory, which only works if it is empty
  Delete "$INSTDIR\ecm-discovery-cli.exe"

  !insertmacro ECM_SAVE_REGISTERS
  !insertmacro ECM_READ_USER_PATH
  ${If} $1 == 1
    ; Rebuild PATH from all entries except the install directory
    StrCpy $2 "$0;"
    StrCpy $3 ""
    StrCpy $4 ""
    StrCpy $6 0
    ${Do}
      StrCpy $5 $2 1
      StrCpy $2 $2 "" 1
      ${If} $5 == ""
        ${Break}
      ${EndIf}
      ${If} $5 == ";"
        ${If} $4 == "$INSTDIR"
          StrCpy $6 1
        ${ElseIf} $4 != ""
          ${If} $3 == ""
            StrCpy $3 $4
          ${Else}
            StrCpy $3 "$3;$4"
          ${EndIf}
        ${EndIf}
        StrCpy $4 ""
      ${Else}
        StrCpy $4 "$4$5"
      ${EndIf}
    ${Loop}
    ${If} $6 == 1
      ${If} $3 == ""
        ; The installer created the user PATH: remove it again
        DeleteRegValue HKCU "Environment" "Path"
      ${Else}
        WriteRegExpandStr HKCU "Environment" "Path" $3
      ${EndIf}
      !insertmacro ECM_BROADCAST_ENVIRONMENT
    ${EndIf}
  ${EndIf}
  !insertmacro ECM_RESTORE_REGISTERS
!macroend
