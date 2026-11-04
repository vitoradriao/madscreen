# Instalador Windows

Template derivado do Tauri CLI **2.11.5**, arquivo
`crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi`, tag
`tauri-cli-v2.11.5`. Licença MIT preservada em `tauri-LICENSE-MIT`.
Ao atualizar o CLI, comparar o template com o upstream antes de empacotar.

Personalizações: manutenção com duas ações consistentes, desinstalação que
encerra sem reinstalar, fechamento do processo na fase de instalação e tema
processual em `theme_sys.cpp`. Os modos `/P`, `/S` e `/UPDATE`, migração WiX,
cancelamento e erros de desinstalação conservam caminhos próprios.

`tools/ui/build-installer-theme.ps1` compila a DLL x86 com MSVC `/W4 /WX` antes
do bundle; NSIS é x86 mesmo ao instalar o app x64. DLL usa somente APIs do
Windows e carrega a fonte com `FR_PRIVATE`, sem instalar fonte no sistema.
Não há polling/timer: notificações de criação/exibição aplicam o tema uma vez
por controle, com bloqueio de reentrada durante o layout.

O nome no cabeçalho é texto nativo OpenAI Sans; a lateral usa bitmap em 4x.
`tools/ui/generate-installer.py` regenera as artes a partir da marca existente.
A fonte vem de `https://cdn.openai.com/common/fonts/openai-sans/v2/OpenAISans-Regular.woff2`;
TTF gerado com fontTools, mantendo metadados. Não substituir por fonte raster.

Validação do fluxo sem registro/processos/instalação pessoal:
`python tools/ui/test_installer_maintenance.py`.
Os testes compilam e executam somente o fluxo NSIS com efeitos externos
substituídos. Não equivalem a upgrade real em VM limpa.
