# Runtime H.264 candidato — build local MSVC

Este pacote de pesquisa não substitui o runtime instalado ou de desenvolvimento.
O build não habilita GPL/nonfree/version3, x264/x265 ou dependências descobertas
automaticamente. É um candidato H.264, com AMF, QuickSync/oneVPL e o experimento
Media Foundation; não é um pacote completo para todos os codecs do FFmpeg.
O resultado negativo de MF não é removido por conseguir compilar a biblioteca.

Requisitos: Windows x64, Visual Studio2022 Build Tools com C++, CMake/Ninja do VS,
Python, tar e Git for Windows nos caminhos verificados pelo script. Destino sem
espaços; pasta nova a cada build. Não exige Docker ou instalação do runtime.

```powershell
tools/encode/build-native-runtime.ps1 -Destination <repo>\target\runtime-native-candidate
```

Para reconstruir offline, preserve os seis arquivos `*.tar.gz` listados em
`native-runtime-sources.json` numa pasta de cache e informe `-SourceCache`.
Cada arquivo é validado por SHA256 antes da extração. O arquivo completo de AMF
é grande; o script extrai somente headers/aviso, mas conserva o tar integral.
O arquivo Meson completo também é conservado, extraindo só os módulos usados
para não depender de symlinks dos testes upstream no Windows.
`-ResumeBuild` permite repetir as etapas no próprio destino somente quando seu
manifesto coincide exatamente; todos os arquivos fonte são novamente validados.
Não use essa opção para comparar dois builds limpos; preserve pastas distintas
nesse caso. Logs anteriores devem ser copiados antes da repetição.

O script compila GNU Make4.4.1, pkgconf2.5.1, oneVPL e FFmpeg das fontes fixadas.
Meson1.9.1 executa a partir de seu arquivo fonte; CMake/Ninja/MSVC vêm do VS.
`VERSION` fixa a versão do FFmpeg para não herdar o Git do aplicativo pai.
Os arquivos gerados Makefile/config.mak são normalizados de `/c/...` para `C:/...`
para o GNU Make nativo. A geração de dependências MSVC usa um script Python
equivalente ao filtro de `showIncludes`, com idioma1033. Nenhum arquivo C/C++ do
FFmpeg, AMF ou oneVPL é alterado. Scripts/logs/configuração ficam no destino.
Os headers públicos/pkg-config são copiados da lista exata de pré-requisitos
de `make -n install-headers`, um arquivo por vez: isso evita truncamento da
linha de instalação pelo GNU Make nativo. O helper só admite destinos do SDK
local e arquivos `.h`/`.pc` das fontes verificadas.

`runtime/bin` contém DLLs, executáveis de inspeção e import libraries.
`runtime/include` contém headers. `runtime/notices` contém avisos, configuração e
manifesto; `inventory.json` registra hashes. Conserve também `archives/`, o script
de build e seus arquivos auxiliares para disponibilizar as fontes correspondentes
e instruções de reconstrução. O código-fonte das dependências não é substituído
por um link para uma branch flutuante.

Para testar lado a lado, defina `MADSCREEN_ENCODER_RUNTIME` somente no processo
de laboratório apontando para `runtime/bin`. O loader valida ABI avcodec62,
avutil60/swresample6 antes de acessar as estruturas. Retorne removendo essa
variável no processo de laboratório; nenhum bundle é sobrescrito.

O aplicativo carrega DLLs dinamicamente. Preserve essa possibilidade de troca,
os headers/import libraries e o procedimento de rebuild aplicável à biblioteca.
Fontes/notices e instruções são insumos de revisão; este documento não declara
que todas as obrigações de distribuição/relinking foram satisfeitas. A licença
do toolchain/CRT Microsoft e termos dos componentes ainda exigem revisão própria.

Verificações antes de qualquer promoção: auditoria de configuração, imports PE
transitivos, ABI C/Rust, codec disponível e testes runtime reais no hardware
disponível. Ausência de AMD/Intel deve ficar explícita. Compilar AMF/QuickSync
não qualifica seus drivers. O build corrente também não prova Windows1903, jogo,
latência/CPU ou fallback CPU aprovado. Não distribuir ou mudar defaults por
ausência de flags bloqueadas.
