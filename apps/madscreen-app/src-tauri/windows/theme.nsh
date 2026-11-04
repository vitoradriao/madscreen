; Font is private to the setup process; nothing is installed into Windows Fonts.
!define MUI_BGCOLOR "000000"
!define MUI_TEXTCOLOR "FAFAFA"
!define MUI_INSTFILESPAGE_COLORS "FAFAFA 141414"
!define MUI_DIRECTORYPAGE_BGCOLOR "141414"
!define MUI_DIRECTORYPAGE_TEXTCOLOR "FAFAFA"
!define MUI_CUSTOMFUNCTION_GUIINIT MadscreenThemeInit
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.MadscreenThemeInit
; Bootstrap with an outline font before the private font is loaded.
SetFont "Segoe UI" 10
InstallColors FAFAFA 141414

!macro ThemeFunctions prefix
Function ${prefix}MadscreenThemeInit
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=madscreen-theme.dll "${__FILEDIR__}\branding\madscreen-theme.dll"
  File /oname=OpenAISans-Regular.ttf "${__FILEDIR__}\fonts\OpenAISans-Regular.ttf"
  System::Call '"$PLUGINSDIR\madscreen-theme.dll"::Initialize(p $HWNDPARENT, w "$PLUGINSDIR\OpenAISans-Regular.ttf") ?c'
FunctionEnd
Function ${prefix}MadscreenThemeApply
  System::Call '"$PLUGINSDIR\madscreen-theme.dll"::Apply(p $HWNDPARENT) ?c'
FunctionEnd
!macroend
!insertmacro ThemeFunctions ""
!insertmacro ThemeFunctions "un."

!macro CloseAppForInstall executableName
  ; Runs after the user starts installation, never while downloading a package.
  nsis_tauri_utils::FindProcessCurrentUser "${executableName}"
  Pop $R0
  ${If} $R0 = 0
    nsis_tauri_utils::KillProcessCurrentUser "${executableName}"
    Pop $R0
    ${If} $R0 != 0
    ${AndIf} $R0 != 2
      Abort "Não foi possível fechar o madscreen. Feche o aplicativo e tente novamente."
    ${EndIf}
    Sleep 500
  ${EndIf}
!macroend
