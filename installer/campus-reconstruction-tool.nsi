Unicode true
SetCompressor /SOLID lzma

!define PRODUCT_NAME "Campus Reconstruction Tool"
!define PRODUCT_VERSION "0.1.0"
!define PRODUCT_PUBLISHER "Campus Reconstruction Tool contributors"
!define PRODUCT_EXE "campus-native.exe"

Name "${PRODUCT_NAME}"
OutFile "..\artifacts\installer\Campus-Reconstruction-Tool-V1-Setup.exe"
InstallDir "$LOCALAPPDATA\Programs\Campus Reconstruction Tool"
RequestExecutionLevel user
ShowInstDetails show
ShowUninstDetails show
Icon "..\src-tauri\icons\icon.ico"
UninstallIcon "..\src-tauri\icons\icon.ico"

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Campus Reconstruction Tool" SEC_MAIN
  SetOutPath "$INSTDIR"
  File "..\native\target\release\campus-native.exe"
  File "..\native\target\release\campus-map.exe"
  File "..\native\target\release\campus-preview.exe"
  File "..\THIRD_PARTY_NOTICES.md"

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
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool"
SectionEnd
