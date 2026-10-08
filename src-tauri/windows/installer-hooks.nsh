; Fino's Explorer integration, installed for the current user like the rest of the app.
;
; Fino shows up in "Open with" for JPEGs and as "Optimize with Fino" on folders, but never
; becomes the default app for photos. (Tauri's own file associations would also set the
; .jpg default wherever the user has not picked an app.) Everything is removed on uninstall.

!define FINO_PROGID "Fino.Jpeg"
!define FINO_EXE "$INSTDIR\${MAINBINARYNAME}.exe"
; SHCNE_ASSOCCHANGED: tells Explorer to reload associations and menus.
!define FINO_REFRESH_SHELL "shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)"

; Language id of NSIS's "Spanish" (installer languages: see tauri.windows.conf.json).
!define FINO_LANG_SPANISH 1034

!macro FINO_OPEN_WITH EXT
  WriteRegStr SHCTX "Software\Classes\.${EXT}\OpenWithProgids" "${FINO_PROGID}" ""
  WriteRegStr SHCTX "Software\Classes\Applications\${MAINBINARYNAME}.exe\SupportedTypes" ".${EXT}" ""
!macroend

!macro FINO_FORGET EXT
  DeleteRegValue SHCTX "Software\Classes\.${EXT}\OpenWithProgids" "${FINO_PROGID}"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "Software\Classes\${FINO_PROGID}" "" "JPEG"
  WriteRegStr SHCTX "Software\Classes\${FINO_PROGID}\DefaultIcon" "" "${FINO_EXE},0"
  WriteRegStr SHCTX "Software\Classes\${FINO_PROGID}\shell\open" "FriendlyAppName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "Software\Classes\${FINO_PROGID}\shell\open\command" "" '"${FINO_EXE}" "%1"'
  WriteRegStr SHCTX "Software\Classes\Applications\${MAINBINARYNAME}.exe" "FriendlyAppName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "Software\Classes\Applications\${MAINBINARYNAME}.exe\shell\open\command" "" '"${FINO_EXE}" "%1"'
  !insertmacro FINO_OPEN_WITH "jpg"
  !insertmacro FINO_OPEN_WITH "jpeg"
  !insertmacro FINO_OPEN_WITH "jpe"
  !insertmacro FINO_OPEN_WITH "jfif"

  ; The folder menu speaks the language the installer ran in (Windows' display language).
  Push $0
  StrCpy $0 "Optimize with Fino"
  StrCmp $LANGUAGE ${FINO_LANG_SPANISH} 0 +2
    StrCpy $0 "Optimizar con Fino"
  WriteRegStr SHCTX "Software\Classes\Directory\shell\Fino" "" "$0"
  Pop $0
  WriteRegStr SHCTX "Software\Classes\Directory\shell\Fino" "Icon" "${FINO_EXE},0"
  WriteRegStr SHCTX "Software\Classes\Directory\shell\Fino\command" "" '"${FINO_EXE}" "%V"'

  ; 0.1.0 had no publisher, so its installer kept its settings under the default one ("fino").
  ; By now its uninstaller has run (the template reads this key to find it).
  DeleteRegKey SHCTX "${FINO_LEGACY_PRODUCTKEY}"
  DeleteRegKey /ifempty SHCTX "Software\fino"

  System::Call "${FINO_REFRESH_SHELL}"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro FINO_FORGET "jpg"
  !insertmacro FINO_FORGET "jpeg"
  !insertmacro FINO_FORGET "jpe"
  !insertmacro FINO_FORGET "jfif"
  DeleteRegKey SHCTX "Software\Classes\${FINO_PROGID}"
  DeleteRegKey SHCTX "Software\Classes\Applications\${MAINBINARYNAME}.exe"
  DeleteRegKey SHCTX "Software\Classes\Directory\shell\Fino"

  System::Call "${FINO_REFRESH_SHELL}"
!macroend
