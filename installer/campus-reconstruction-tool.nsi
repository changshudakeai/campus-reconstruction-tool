Unicode true
SetCompressor /SOLID lzma

!define PRODUCT_NAME "Campus Reconstruction Tool"
!ifndef PRODUCT_VERSION
  !define PRODUCT_VERSION "1.1.0"
!endif
!ifndef PAYLOAD_DIR
  !error "PAYLOAD_DIR must identify the already-tested V1.1 binaries"
!endif
!ifndef CANDIDATE_MANIFEST
  !error "CANDIDATE_MANIFEST must identify the traceable candidate manifest"
!endif
!ifndef RELEASE_NOTES
  !error "RELEASE_NOTES must identify the unsigned release guidance"
!endif
!ifndef OUTPUT_FILE
  !error "OUTPUT_FILE must identify the immutable candidate installer"
!endif
!define PRODUCT_PUBLISHER "Campus Reconstruction Tool contributors"
!define PRODUCT_EXE "campus-native.exe"

Name "${PRODUCT_NAME}"
OutFile "${OUTPUT_FILE}"
VIProductVersion "1.1.0.0"
VIAddVersionKey /LANG=1033 "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${PRODUCT_VERSION}"
VIAddVersionKey /LANG=1033 "FileVersion" "${PRODUCT_VERSION}"
VIAddVersionKey /LANG=1033 "FileDescription" "${PRODUCT_NAME} V1.1.0 Windows 11 x64 installer"
InstallDir "$LOCALAPPDATA\Programs\Campus Reconstruction Tool"
RequestExecutionLevel user
ShowInstDetails show
ShowUninstDetails show
Icon "..\native\assets\icon.ico"
UninstallIcon "..\native\assets\icon.ico"

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Campus Reconstruction Tool" SEC_MAIN
  SetOutPath "$INSTDIR"
  File "${PAYLOAD_DIR}\campus-native.exe"
  File "${PAYLOAD_DIR}\campus-map.exe"
  File "${PAYLOAD_DIR}\campus-preview.exe"
  File "..\THIRD_PARTY_NOTICES.md"
  File /oname=release-candidate.json "${CANDIDATE_MANIFEST}"
  File /oname=V1.1.0-RELEASE-NOTES.md "${RELEASE_NOTES}"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\Campus Reconstruction Tool"
  CreateShortcut "$SMPROGRAMS\Campus Reconstruction Tool\Campus Reconstruction Tool.lnk" "$INSTDIR\${PRODUCT_EXE}"
  CreateShortcut "$SMPROGRAMS\Campus Reconstruction Tool\卸载.lnk" "$INSTDIR\Uninstall.exe"

  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool" "Publisher" "${PRODUCT_PUBLISHER}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool" "DisplayIcon" "$INSTDIR\${PRODUCT_EXE}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\Campus Reconstruction Tool\Campus Reconstruction Tool.lnk"
  Delete "$SMPROGRAMS\Campus Reconstruction Tool\卸载.lnk"
  RMDir "$SMPROGRAMS\Campus Reconstruction Tool"
  Delete "$INSTDIR\campus-native.exe"
  Delete "$INSTDIR\campus-map.exe"
  Delete "$INSTDIR\campus-preview.exe"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.md"
  Delete "$INSTDIR\release-candidate.json"
  Delete "$INSTDIR\V1.1.0-RELEASE-NOTES.md"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool"
SectionEnd
