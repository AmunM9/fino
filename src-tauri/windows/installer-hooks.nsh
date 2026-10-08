; Fino's Explorer integration, installed for the current user like the rest of the app.
;
; Fino shows up in "Open with" for JPEGs and as "Optimizar con Fino" on folders, but never
; becomes the default app for photos. (Tauri's own file associations would also set the
; .jpg default wherever the user has not picked an app.) Everything is removed on uninstall.

!define FINO_PROGID "Fino.Jpeg"
!define FINO_EXE "$INSTDIR\${MAINBINARYNAME}.exe"
; SHCNE_ASSOCCHANGED: tells Explorer to reload associations and menus.
!define FINO_REFRESH_SHELL "shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)"

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

  WriteRegStr SHCTX "Software\Classes\Directory\shell\Fino" "" "Optimizar con Fino"
  WriteRegStr SHCTX "Software\Classes\Directory\shell\Fino" "Icon" "${FINO_EXE},0"
  WriteRegStr SHCTX "Software\Classes\Directory\shell\Fino\command" "" '"${FINO_EXE}" "%V"'

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
