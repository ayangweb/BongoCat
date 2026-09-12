Unicode true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!ifndef INPUT_DIRECTORY
!error "INPUT_DIRECTORY must name the signed product payload"
!endif
!ifndef OUTPUT_FILE
!error "OUTPUT_FILE must name the installer artifact"
!endif
!ifndef PRODUCT_VERSION
!error "PRODUCT_VERSION must be supplied by the release pipeline"
!endif

!define PRODUCT_NAME "BongoCat"
!define PRODUCT_REGISTRY_KEY "Software\BongoCat"
!define UNINSTALL_REGISTRY_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\BongoCat"

Name "${PRODUCT_NAME} ${PRODUCT_VERSION}"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\BongoCat"
ShowInstDetails show
ShowUninstDetails show

Section "BongoCat (required)" SectionMain
  StrCmp $INSTDIR "$LOCALAPPDATA\Programs\BongoCat" 0 unexpected_install_directory

  ; A release payload is self-contained. Removing only this fixed product root
  ; prevents stale binaries from surviving an in-place upgrade.
  RMDir /r "$INSTDIR"
  SetOutPath "$INSTDIR"
  File /r "${INPUT_DIRECTORY}\*"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "${PRODUCT_REGISTRY_KEY}" "InstallDirectory" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_REGISTRY_KEY}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKCU "${UNINSTALL_REGISTRY_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_REGISTRY_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_REGISTRY_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${UNINSTALL_REGISTRY_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_REGISTRY_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  StrCmp $INSTDIR "$LOCALAPPDATA\Programs\BongoCat" 0 unexpected_install_directory

  Delete "$INSTDIR\Uninstall.exe"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTALL_REGISTRY_KEY}"
  DeleteRegKey HKCU "${PRODUCT_REGISTRY_KEY}"
  Return

unexpected_install_directory:
  MessageBox MB_ICONSTOP "BongoCat can only uninstall its current-user product directory."
  Abort
SectionEnd
