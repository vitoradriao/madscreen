; Substitui a edicao antiga pelo app unico. /S conserva os dados do usuario no desinstalador Tauri.
; O app importa o localStorage antes de criar a primeira janela; nao remover as pastas de dados.
!include LogicLib.nsh
!include "${__FILEDIR__}\theme.nsh"

; Identidade visual e textos; o template oficial conserva paginas, upgrade e /P /S.
!define MUI_HEADERIMAGE_RIGHT
!define MUI_WELCOMEPAGE_TITLE "Instalar o madscreen"
!define MUI_WELCOMEPAGE_TEXT "Este assistente instala o madscreen neste computador.$\r$\n$\r$\nCompartilhe a tela e o áudio de um aplicativo ou assista às lives dos seus canais.$\r$\n$\r$\nClique em Próximo para continuar."
!define MUI_FINISHPAGE_TITLE "Instalação concluída"
!define MUI_FINISHPAGE_TEXT "O madscreen foi instalado.$\r$\n$\r$\nClique em Concluir para fechar este assistente."
!define MUI_FINISHPAGE_RUN_TEXT "Abrir o madscreen"

!macro NSIS_HOOK_PREINSTALL
  Push $R0
  Push $R1
  Push $R2
  ; NVIDIA deixou de ser requisito: disponibilidade dos encoders e testada pelo engine.
  ; Windows 10 1903+: a disponibilidade real de WGC e conferida ao iniciar a captura.
  ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows NT\CurrentVersion" "CurrentBuildNumber"
  ${If} $R0 < 18362
    SetErrorLevel 1
    Abort "O madscreen requer Windows 10 1903 (build 18362) de 64 bits ou mais recente, com captura de tela disponivel."
  ${EndIf}
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\madscreen viewer" "UninstallString"
  ${If} $R0 != ""
    ReadRegStr $R1 HKCU "Software\madscreen\madscreen viewer" ""
    ${If} $R1 == ""
      SetErrorLevel 1
      Abort "Nao foi possivel localizar a instalacao antiga. Desinstale-a sem apagar os dados e tente novamente."
    ${EndIf}
    DetailPrint "Substituindo a edicao antiga e preservando login e preferencias..."
    ClearErrors
    ; _?= evita o relancamento em background e permite aguardar o fim da desinstalacao.
    ExecWait '$R0 /S _?=$R1' $R2
    ${If} ${Errors}
      SetErrorLevel 1
      Abort "Nao foi possivel remover a edicao antiga. Feche o app e tente novamente."
    ${EndIf}
    ${If} $R2 != 0
      SetErrorLevel 1
      Abort "A desinstalacao antiga nao terminou. Feche o app e tente novamente."
    ${EndIf}
    ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\madscreen viewer" "UninstallString"
    ${If} $R0 != ""
      SetErrorLevel 1
      Abort "A edicao antiga ainda esta instalada. Desinstale-a sem apagar os dados e tente novamente."
    ${EndIf}
    ; O desinstalador executado no lugar nao consegue apagar a si mesmo. Sem remocao recursiva.
    Delete "$R1\uninstall.exe"
    RMDir "$R1"
  ${EndIf}
  Pop $R2
  Pop $R1
  Pop $R0
  ClearErrors
!macroend
