# Runtime de codificação distribuído

O instalador inclui FFmpeg 8.1.2 full shared de Gyan Doshi, com libx264,
sob GPL v3 ou posterior. LICENSE e README.txt do pacote acompanham as DLLs.
Configuração e fontes correspondentes: https://www.gyan.dev/ffmpeg/builds/
e https://github.com/FFmpeg/FFmpeg/tree/n8.1.2 . Bibliotecas externas e suas
fontes são relacionadas no README do fornecedor e na configuração de build.

O carregamento é dinâmico; os drivers NVIDIA/AMD/Intel continuam sendo fornecidos
pelos fabricantes. AMF e QuickSync não são implementações de software do app.

Para reproduzir o pacote, execute tools/encode/prepare-runtime.ps1; a versão e o
SHA-256 são fixos no script. Distribuições do app com esse runtime devem cumprir
as condições da GPL aplicáveis ao conjunto, incluindo disponibilização das fontes
correspondentes e instruções de compilação. Não substituir por build nonfree.
