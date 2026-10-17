; Tauri NSIS installer hooks (bundle.windows.nsis.installerHooks).
;
; NSIS upgrades overwrite files one by one and never delete files that the new
; version no longer ships. In /UPDATE mode (the in-app updater) the old version
; is not uninstalled first either. So files dropped from the Python runtime
; (pruned packages, old ggml-*.dll copies) and runtime-generated __pycache__
; dirs would pile up in $INSTDIR across upgrades. Wipe the two generated
; resource trees before the new files are copied.
;
; Only install-tree resources are touched. Models, settings, history and the
; faster-whisper HF cache all live under %APPDATA%\voicewave\localcore, never
; under $INSTDIR.

!macro VW_REMOVE_STALE_RUNTIME
  ; Each removal is gated on a file only a real VoiceWave runtime contains, so a
  ; blank/odd $INSTDIR can never turn this into a delete of something else.
  ${If} $INSTDIR != ""
    ${If} ${FileExists} "$INSTDIR\faster-whisper\python\python.exe"
      RMDir /r "$INSTDIR\faster-whisper\python"
    ${EndIf}
    ${If} ${FileExists} "$INSTDIR\asr-worker\vk\voicewave-asr-vk.exe"
      RMDir /r "$INSTDIR\asr-worker\vk"
    ${EndIf}
    ${If} ${FileExists} "$INSTDIR\asr-worker\cpu\voicewave-asr-cpu.exe"
      RMDir /r "$INSTDIR\asr-worker\cpu"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; This hook runs before Tauri's own running-app check. Run that check first
  ; (it asks to close the app, and aborts the install if the user declines) so
  ; we never strip the runtime out from under a live app or a cancelled install.
  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  !insertmacro VW_REMOVE_STALE_RUNTIME
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; The uninstaller only deletes the files it was built with, so __pycache__
  ; written by the running app would leave the runtime folder behind.
  ${If} $INSTDIR != ""
    ${If} ${FileExists} "$INSTDIR\faster-whisper\python\*.*"
      RMDir /r "$INSTDIR\faster-whisper\python"
    ${EndIf}
  ${EndIf}
!macroend
