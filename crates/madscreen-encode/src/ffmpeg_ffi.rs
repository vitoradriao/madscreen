//! FFmpeg 8.x: AMF, QuickSync e x264. ABI 62/60 verificada antes de acessar structs.
//! Entrada NV12 com staging explícito; somente o backend NVENC é zero-copy atualmente.
//! O candidato OpenH264 (libopenh264, opt-in) só aceita I420: o readback NV12 é convertido em quadros
//! YUV420P pré-alocados com os pitches do FFmpeg.
#![allow(unsafe_code)]
use crate::{
    AuBuffer, EncodeError, EncoderConfig, FrameKind, FrameRate, FrameTiming, Result, VideoCodec,
    backend::BackendKind,
    convert::nv12_i420::{I420Planes, nv12_to_i420},
    pipeline::PendingQueue,
    platform::{dll_ffi::Library, qpc},
};
use madscreen_capture::wgc::GpuDevice;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
    sync::Arc,
    time::{Duration, Instant},
};
use windows::{Win32::Graphics::Direct3D11::*, core::s};

// Prefixos públicos de libavutil/frame.h e libavcodec/packet.h (n8.0).
// Nunca alocar essas structs em Rust: campos posteriores pertencem à biblioteca.
#[repr(C)]
struct Frame {
    data: [*mut u8; 8],
    linesize: [i32; 8],
    extended_data: *mut *mut u8,
    width: i32,
    height: i32,
    nb_samples: i32,
    format: i32,
    pict_type: i32,
    sample_aspect_ratio: [i32; 2],
    pts: i64,
    // Public AVFrame fields up to duration, avutil 60 x64. Never allocate in Rust.
    _reserved_to_duration: [u8; 264],
    duration: i64,
}
#[repr(C)]
struct Packet {
    buf: *mut c_void,
    pts: i64,
    dts: i64,
    data: *mut u8,
    size: i32,
    stream_index: i32,
    flags: i32,
}
type Context = c_void;
type Codec = c_void;
struct Api {
    find: unsafe extern "C" fn(*const c_char) -> *const Codec,
    alloc: unsafe extern "C" fn(*const Codec) -> *mut Context,
    free: unsafe extern "C" fn(*mut *mut Context),
    open: unsafe extern "C" fn(*mut Context, *const Codec, *mut *mut c_void) -> i32,
    send: unsafe extern "C" fn(*mut Context, *const Frame) -> i32,
    receive: unsafe extern "C" fn(*mut Context, *mut Packet) -> i32,
    frame_alloc: unsafe extern "C" fn() -> *mut Frame,
    frame_free: unsafe extern "C" fn(*mut *mut Frame),
    frame_buffer: unsafe extern "C" fn(*mut Frame, i32) -> i32,
    frame_writable: unsafe extern "C" fn(*mut Frame) -> i32,
    packet_alloc: unsafe extern "C" fn() -> *mut Packet,
    packet_free: unsafe extern "C" fn(*mut *mut Packet),
    packet_unref: unsafe extern "C" fn(*mut Packet),
    option: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, i32) -> i32,
    pixel_format: unsafe extern "C" fn(*const c_char) -> i32,
    strerror: unsafe extern "C" fn(i32, *mut c_char, usize) -> i32,
    _codec: Library,
    _util: Library,
}
// SAFETY: tabela imutável de funções C; o loader Windows permite uso/liberação em qualquer thread.
unsafe impl Send for Api {}
// SAFETY: apenas símbolos imutáveis são compartilhados; contextos são exclusivos de Session.
unsafe impl Sync for Api {}
impl Api {
    fn load() -> Result<Arc<Self>> {
        let exe = std::env::current_exe().map_err(|e| unavailable(e.to_string()))?;
        let parent = exe
            .parent()
            .ok_or_else(|| unavailable("pasta do executável ausente"))?;
        // O instalador usa resources/encoder-runtime; desenvolvimento também aceita DLLs ao lado do engine.
        let override_dir =
            std::env::var_os("MADSCREEN_ENCODER_RUNTIME").map(std::path::PathBuf::from);
        let directory = override_dir.into_iter().chain([parent.join("encoder-runtime"), parent.to_path_buf()])
            .into_iter().find(|p| p.join("avcodec-62.dll").is_file())
            .ok_or_else(|| unavailable("runtime FFmpeg ausente; execute tools/encode/prepare-runtime.ps1 e copie as DLLs para encoder-runtime ao lado do engine"))?;
        let load = |name: &str| Library::load_path(&directory.join(name));
        // avcodec depende de avutil; carregar primeiro o arquivo de caminho absoluto.
        let util = load("avutil-60.dll")?;
        let codec = load("avcodec-62.dll")?;
        type Version = unsafe extern "C" fn() -> u32;
        // SAFETY: símbolos oficiais; DLLs vivas. Só acessamos os prefixos após validar os majors.
        unsafe {
            let cv = codec.symbol::<Version>(s!("avcodec_version"))?() >> 16;
            let uv = util.symbol::<Version>(s!("avutil_version"))?() >> 16;
            if cv != 62 || uv != 60 {
                return Err(unavailable(
                    "ABI FFmpeg incompatível; exige avcodec 62 / avutil 60",
                ));
            }
            Ok(Arc::new(Self {
                find: codec.symbol(s!("avcodec_find_encoder_by_name"))?,
                alloc: codec.symbol(s!("avcodec_alloc_context3"))?,
                free: codec.symbol(s!("avcodec_free_context"))?,
                open: codec.symbol(s!("avcodec_open2"))?,
                send: codec.symbol(s!("avcodec_send_frame"))?,
                receive: codec.symbol(s!("avcodec_receive_packet"))?,
                frame_alloc: util.symbol(s!("av_frame_alloc"))?,
                frame_free: util.symbol(s!("av_frame_free"))?,
                frame_buffer: util.symbol(s!("av_frame_get_buffer"))?,
                frame_writable: util.symbol(s!("av_frame_is_writable"))?,
                packet_alloc: codec.symbol(s!("av_packet_alloc"))?,
                packet_free: codec.symbol(s!("av_packet_free"))?,
                packet_unref: codec.symbol(s!("av_packet_unref"))?,
                option: util.symbol(s!("av_opt_set"))?,
                pixel_format: util.symbol(s!("av_get_pix_fmt"))?,
                strerror: util.symbol(s!("av_strerror"))?,
                _codec: codec,
                _util: util,
            }))
        }
    }
    fn check(&self, status: i32, operation: &str) -> Result<()> {
        if status >= 0 {
            return Ok(());
        }
        let mut message = [0i8; 256];
        // SAFETY: buffer de saída válido, tamanho em bytes; av_strerror termina em NUL.
        unsafe {
            (self.strerror)(status, message.as_mut_ptr(), message.len());
        }
        // SAFETY: buffer foi zerado e a função preserva terminação.
        let message = unsafe { CStr::from_ptr(message.as_ptr()) }.to_string_lossy();
        Err(unavailable(format!("{operation}: {message} ({status})")))
    }
    fn set(&self, context: *mut Context, key: &str, value: &str) -> Result<()> {
        let key_c = CString::new(key).map_err(|_| unavailable("opção com NUL"))?;
        let value_c = CString::new(value).map_err(|_| unavailable("valor com NUL"))?;
        // SAFETY: strings NUL e contexto vivo. SEARCH_CHILDREN inclui opções específicas do codec.
        self.check(
            unsafe { (self.option)(context, key_c.as_ptr(), value_c.as_ptr(), 1) },
            key,
        )
    }
}
/// AMF (amfenc.c) não relê a taxa após abrir; MF/OpenH264 são candidatos de laboratório.
fn supports_inplace_rate(kind: BackendKind) -> bool {
    matches!(kind, BackendKind::X264 | BackendKind::Qsv)
}
fn unavailable(message: impl Into<String>) -> EncodeError {
    EncodeError::EncoderUnavailable(message.into())
}

fn configure_amf(api: &Api, context: *mut Context) -> Result<()> {
    let set = |key: &str, value: &str| api.set(context, key, value);
    set("usage", "ultralowlatency")?;
    set("quality", "speed")?;
    set("rc", "cbr")?;
    set("preanalysis", "0")?;
    // FFmpeg 8.1.2 amfenc.c mantém uma entrada de atraso sem LOW_DELAY,
    // mesmo sem B-frames. Um probe de um quadro nunca sairia de EAGAIN.
    set("flags", "+low_delay")?;
    // AV_PICTURE_TYPE_I sozinho pede I, não IDR nem reinserção de SPS/PPS.
    // Necessário tanto no probe quanto nos pedidos de recuperação do viewer.
    set("forced_idr", "1")?;
    set("async_depth", "1")?;
    Ok(())
}

struct Session {
    api: Arc<Api>,
    context: *mut Context,
    packet: *mut Packet,
    frames: Vec<*mut Frame>,
    /// Quadros YUV420P (OpenH264) em vez de NV12.
    i420: bool,
}
impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: todos os objetos foram alocados pela mesma API; funções aceitam ponteiro nulo.
        unsafe {
            (self.api.free)(&mut self.context);
            for frame in &mut self.frames {
                (self.api.frame_free)(frame);
            }
            (self.api.packet_free)(&mut self.packet);
        }
    }
}
impl Session {
    fn new(api: Arc<Api>, config: &EncoderConfig, kind: BackendKind, slots: usize) -> Result<Self> {
        let name = match kind {
            BackendKind::Amf => c"h264_amf",
            BackendKind::Qsv => c"h264_qsv",
            BackendKind::X264 => c"libx264",
            BackendKind::Mf => c"h264_mf",
            BackendKind::OpenH264 => c"libopenh264",
            _ => return Err(unavailable("backend FFmpeg inválido")),
        };
        // SAFETY: nome NUL; retornos checados antes de uso, propriedade transferida para Session.
        let codec = unsafe { (api.find)(name.as_ptr()) };
        if codec.is_null() {
            return Err(unavailable(format!("{} ausente no runtime", kind.name())));
        }
        let mut session = Self {
            context: unsafe { (api.alloc)(codec) },
            packet: ptr::null_mut(),
            frames: Vec::with_capacity(slots),
            i420: kind == BackendKind::OpenH264,
            api,
        };
        if session.context.is_null() {
            return Err(unavailable("avcodec_alloc_context3 falhou"));
        }
        let set = |key: &str, value: &str| session.api.set(session.context, key, value);
        set("video_size", &format!("{}x{}", config.width, config.height))?;
        set(
            "pixel_format",
            if session.i420 { "yuv420p" } else { "nv12" },
        )?;
        set(
            "time_base",
            &format!("{}/{}", config.fps.den, config.fps.num),
        )?;
        set("b", &config.bitrate_bps.to_string())?;
        set("maxrate", &config.bitrate_bps.to_string())?;
        set("bufsize", &config.vbv_bits()?.to_string())?;
        set("bf", "0")?;
        set("refs", "1")?;
        set("g", "2147483647")?;
        // Limitar uso de CPU; nunca criar uma thread por núcleo da máquina.
        set("threads", "2")?;
        set("color_range", "tv")?;
        set("colorspace", "bt709")?;
        set("color_primaries", "bt709")?;
        set("color_trc", "bt709")?;
        match kind {
            BackendKind::Amf => {
                configure_amf(&session.api, session.context)?;
            }
            BackendKind::Qsv => {
                set("preset", "veryfast")?;
                set("async_depth", "1")?;
                set("look_ahead", "0")?;
                set("forced_idr", "1")?;
            }
            BackendKind::Mf => {
                set("hw_encoding", "false")?;
                set("rate_control", "ld_vbr")?;
                set("scenario", "display_remoting")?;
                // mfenc.c enables CODECAPI_AVLowLatencyMode only with LOW_DELAY.
                // The rate-control name alone does not request this MFT property.
                set("flags", "+low_delay")?;
            }
            BackendKind::OpenH264 => {
                // libopenh264enc.c (760d1060): o padrão é rc_mode=quality; frame skip devolveria
                // pacote vazio, então fica desligado de forma explícita.
                set("rc_mode", "bitrate")?;
                set("allow_skip_frames", "0")?;
                // O probe medido usava uma fatia por thread (threads=2); com fatia única o OpenH264
                // ignora iMultipleThreadIdc. `slices` é opção genérica do AVCodecContext.
                // O wrapper também força BackgroundDetection/AdaptiveQuant=1 (igual ao probe), mas deixa
                // iComplexityMode e SceneChangeDetect nos padrões do SDK: sem opção para ajustá-los.
                set("slices", "2")?;
            }
            BackendKind::X264 => {
                set("preset", "ultrafast")?;
                set("tune", "zerolatency")?;
                set("forced-idr", "1")?;
                set(
                    "x264-params",
                    if config.h264_profile == crate::config::H264Profile::ConstrainedBaseline {
                        "repeat-headers=1:annexb=1:scenecut=0:sliced-threads=1:cabac=0:8x8dct=0"
                    } else if config.h264_profile == crate::config::H264Profile::Main {
                        "repeat-headers=1:annexb=1:scenecut=0:sliced-threads=1:cabac=1:8x8dct=0"
                    } else {
                        "repeat-headers=1:annexb=1:scenecut=0:sliced-threads=1:cabac=1:8x8dct=1"
                    },
                )?;
            }
            _ => {}
        }
        // Por último: o usage ultralowlatency do AMF volta o perfil para Main se vier depois.
        set(
            "profile",
            match (kind, config.h264_profile) {
                (BackendKind::Mf, crate::config::H264Profile::ConstrainedBaseline) => "66",
                (BackendKind::Mf, crate::config::H264Profile::Main) => "77",
                (BackendKind::Mf, _) => "100",
                (BackendKind::OpenH264, crate::config::H264Profile::ConstrainedBaseline) => {
                    "constrained_baseline"
                }
                (BackendKind::OpenH264, crate::config::H264Profile::Main) => "main",
                (BackendKind::OpenH264, _) => "high",
                (_, crate::config::H264Profile::ConstrainedBaseline) => "baseline",
                (_, crate::config::H264Profile::Main) => "main",
                _ => "high",
            },
        )?;
        // libopenh264 não lê `level`: o nível real vem do SPS e é lido pelo frame de prova.
        if let Some(level) = config.h264_level.filter(|_| kind != BackendKind::OpenH264) {
            set(
                "level",
                &crate::backend::ffmpeg_h264_level_value(kind, level),
            )?;
        }
        // SAFETY: contexto configurado, codec válido, dicionário opcional nulo.
        session.api.check(
            unsafe { (session.api.open)(session.context, codec, ptr::null_mut()) },
            "avcodec_open2",
        )?;
        // SAFETY: alocador da API, retorno checado abaixo.
        session.packet = unsafe { (session.api.packet_alloc)() };
        if session.packet.is_null() {
            return Err(unavailable("av_packet_alloc falhou"));
        }
        // Uma referência anterior + entradas em voo + uma superfície livre, sem renovar por frame.
        for _ in 0..slots + 2 {
            // SAFETY: objetos alocados pela biblioteca; só o prefixo ABI validado é acessado.
            let frame = unsafe { (session.api.frame_alloc)() };
            if frame.is_null() {
                return Err(unavailable("av_frame_alloc falhou"));
            }
            session.frames.push(frame);
            // SAFETY: frame válido, dimensões validadas e formato consultado na API carregada.
            unsafe {
                (*frame).width = config.width as i32;
                (*frame).height = config.height as i32;
                (*frame).format = (session.api.pixel_format)(if session.i420 {
                    c"yuv420p".as_ptr()
                } else {
                    c"nv12".as_ptr()
                });
                (*frame).duration = 1;
            }
            // SAFETY: aloca os planos com strides e padding controlados pelo FFmpeg.
            session.api.check(
                unsafe { (session.api.frame_buffer)(frame, 32) },
                "av_frame_get_buffer",
            )?;
        }
        Ok(session)
    }
    fn smoke(&mut self, config: &EncoderConfig) -> Result<Option<String>> {
        let frame = self.frames[0];
        // (plano, linhas, valor, bytes por linha): NV12 = Y + UV intercalado; I420 = Y + U + V.
        let (width, height) = (config.width as usize, config.height as usize);
        let planes: &[(usize, usize, u8, usize)] = if self.i420 {
            &[
                (0, height, 16, width),
                (1, height / 2, 128, width / 2),
                (2, height / 2, 128, width / 2),
            ]
        } else {
            &[(0, height, 16, width), (1, height / 2, 128, width)]
        };
        // SAFETY: planos alocados em new; inicializa apenas linhas válidas respeitando stride.
        unsafe {
            for &(plane, rows, value, bytes) in planes {
                for y in 0..rows {
                    ptr::write_bytes(
                        (*frame).data[plane].add(y * (*frame).linesize[plane] as usize),
                        value,
                        bytes,
                    );
                }
            }
            (*frame).pts = 0;
            (*frame).pict_type = 1;
            self.api
                .check((self.api.send)(self.context, frame), "probe send")?;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            // SAFETY: packet/context vivos e exclusivos.
            let status = unsafe { (self.api.receive)(self.context, self.packet) };
            if status == -11 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            self.api.check(status, "probe receive")?;
            // SAFETY: receive bem-sucedido fornece dados válidos até unref.
            let packet = unsafe { &*self.packet };
            if packet.size <= 0 || packet.data.is_null() {
                return Err(unavailable("probe sem bitstream"));
            }
            // SAFETY: intervalo fornecido pelo packet vivo.
            let data = unsafe { std::slice::from_raw_parts(packet.data, packet.size as usize) };
            let info = crate::bitstream::inspect_annex_b(VideoCodec::H264, data);
            if !info.key || !info.sps || !info.pps {
                return Err(unavailable("probe não produziu IDR Annex B com SPS/PPS"));
            }
            return Ok(crate::bitstream::h264_access_unit_profile_level_id(data));
        }
    }
}
/// Converte o mapeamento NV12 do staging em um quadro YUV420P do FFmpeg, sem alocar.
///
/// Toda a geometria é validada ANTES de criar qualquer slice: ponteiros não nulos, dimensões pares,
/// pitches suficientes, comprimentos com `checked_mul` e no máximo `isize::MAX`. O conversor seguro
/// (`nv12_to_i420`) não pode corrigir um slice já malformado, por isso a checagem fica aqui.
///
/// # Safety
/// `mapped` deve ser um Map de leitura vivo de uma textura NV12 `width`x`height` (UV logo após
/// `height * RowPitch`, como o caminho NV12 e o D3D11 definem para NV12 em staging) com pelo menos
/// `RowPitch * height * 3 / 2` bytes legíveis. `frame` deve ser um AVFrame YUV420P `width`x`height`
/// cujos buffers vieram de `av_frame_get_buffer`, exclusivo desta chamada. Essa função do FFmpeg
/// aloca cada plano com ao menos `linesize[i] * linhas` bytes (linhas = `height` para Y e
/// `height / 2` para U/V; o padding só aumenta), e `linesize[i]` é o pitch do plano `i`.
unsafe fn convert_to_i420(
    config: &EncoderConfig,
    mapped: &D3D11_MAPPED_SUBRESOURCE,
    frame: *mut Frame,
) -> Result<()> {
    let (width, height) = (config.width as usize, config.height as usize);
    if width < 2 || height < 2 || width % 2 != 0 || height % 2 != 0 {
        return Err(EncodeError::InvalidConfig("dimensões I420 devem ser pares"));
    }
    let pitch = mapped.RowPitch as usize;
    if mapped.pData.is_null() || pitch < width {
        return Err(EncodeError::InvalidConfig("mapeamento NV12 inválido"));
    }
    // Comprimentos em bytes: nunca estourar usize nem passar de isize::MAX (limite de slice).
    let length = |stride: usize, rows: usize| {
        stride
            .checked_mul(rows)
            .filter(|&bytes| isize::try_from(bytes).is_ok())
            .ok_or(EncodeError::InvalidConfig("plano grande demais"))
    };
    let source_len = length(pitch, height)?
        .checked_mul(3)
        .map(|bytes| bytes / 2)
        .ok_or(EncodeError::InvalidConfig("mapeamento NV12 grande demais"))?;
    // SAFETY: `frame` é um AVFrame vivo e exclusivo (contrato acima); só lemos campos do prefixo ABI.
    let (data, linesize, frame_width, frame_height) = unsafe {
        (
            (*frame).data,
            (*frame).linesize,
            (*frame).width,
            (*frame).height,
        )
    };
    if frame_width != config.width as i32 || frame_height != config.height as i32 {
        return Err(EncodeError::InvalidConfig(
            "quadro I420 com dimensões diferentes",
        ));
    }
    let widths = [width, width / 2, width / 2];
    let rows = [height, height / 2, height / 2];
    let mut strides = [0usize; 3];
    let mut lengths = [0usize; 3];
    for plane in 0..3 {
        strides[plane] = usize::try_from(linesize[plane])
            .ok()
            .filter(|&stride| stride >= widths[plane])
            .ok_or(EncodeError::InvalidConfig("pitch I420 menor que a largura"))?;
        if data[plane].is_null() {
            return Err(EncodeError::InvalidConfig("plano I420 nulo"));
        }
        lengths[plane] = length(strides[plane], rows[plane])?;
    }
    // SAFETY: ponteiros não nulos e comprimentos validados acima; os três planos de
    // `av_frame_get_buffer` e o mapeamento D3D11 são regiões distintas, sem aliasing.
    let (source, mut planes) = unsafe {
        (
            std::slice::from_raw_parts(mapped.pData.cast::<u8>(), source_len),
            I420Planes {
                y: std::slice::from_raw_parts_mut(data[0], lengths[0]),
                y_stride: strides[0],
                u: std::slice::from_raw_parts_mut(data[1], lengths[1]),
                u_stride: strides[1],
                v: std::slice::from_raw_parts_mut(data[2], lengths[2]),
                v_stride: strides[2],
            },
        )
    };
    nv12_to_i420(source, pitch, width, height, &mut planes)
}
struct Slot {
    texture: ID3D11Texture2D,
    staging: ID3D11Texture2D,
}
struct Pending {
    timing: FrameTiming,
    index: u64,
}
pub(crate) struct FfmpegEncoder {
    session: Session,
    gpu: GpuDevice,
    config: EncoderConfig,
    kind: BackendKind,
    slots: Vec<Slot>,
    pending: PendingQueue<Pending>,
    index: u64,
    keyframe: bool,
    eos: bool,
    /// profile-level-id lido do SPS do quadro de prova; o driver pode subir o nível pedido.
    bitstream_profile_level_id: Option<String>,
    /// Troca de bitrate na sessão aberta (x264/QuickSync); desliga após a primeira falha.
    inplace_rate: bool,
    /// QuickSync só valida o Reset no próximo `avcodec_send_frame`.
    rate_unverified: bool,
}
// SAFETY: sessão exclusiva; DLLs/COM retidos, context D3D11 protegido; acesso sempre por &mut.
unsafe impl Send for FfmpegEncoder {}
impl FfmpegEncoder {
    pub fn new(
        gpu: &GpuDevice,
        mut config: EncoderConfig,
        textures: &[ID3D11Texture2D],
        kind: BackendKind,
    ) -> Result<Self> {
        config.validate()?;
        config.clamp_h264_bitrate();
        if config.codec != VideoCodec::H264 {
            return Err(unavailable(
                "AMF/QuickSync/x264 suportam H.264 neste pipeline",
            ));
        }
        if textures.is_empty() {
            return Err(unavailable("anel vazio"));
        }
        let api = Api::load()?;
        // Abrir o codec não basta: driver sem hardware/sessão funcional cai para a alternativa.
        let mut probe = Session::new(api.clone(), &config, kind, 1)?;
        let bitstream_profile_level_id = probe.smoke(&config)?;
        drop(probe);
        let session = Session::new(api, &config, kind, textures.len())?;
        let mut slots = Vec::with_capacity(textures.len());
        for texture in textures {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            // SAFETY: textura viva e descriptor de saída válido.
            unsafe {
                texture.GetDesc(&mut desc);
            }
            if desc.Format != windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12
                || desc.Width != config.width
                || desc.Height != config.height
            {
                return Err(unavailable(
                    "superfície FFmpeg precisa ser NV12 nas dimensões da saída",
                ));
            }
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.MiscFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            let mut staging = None;
            // SAFETY: descriptor staging sem bind, saída local.
            unsafe {
                gpu.device
                    .CreateTexture2D(&desc, None, Some(&mut staging))?;
            }
            slots.push(Slot {
                texture: texture.clone(),
                staging: staging.ok_or_else(|| unavailable("staging nulo"))?,
            });
        }
        tracing::warn!(
            encoder = kind.name(),
            "Backend de compatibilidade com readback NV12; CPU e impacto no jogo dependem do PC"
        );
        Ok(Self {
            session,
            gpu: gpu.clone(),
            config,
            kind,
            pending: PendingQueue::new(slots.len()),
            slots,
            index: 0,
            keyframe: true,
            eos: false,
            bitstream_profile_level_id,
            inplace_rate: supports_inplace_rate(kind),
            rate_unverified: false,
        })
    }
    /// Verdadeiro quando a próxima troca de bitrate reabre a sessão (IDR + pausa de centenas de ms).
    pub fn rate_change_reopens(&self) -> bool {
        !self.inplace_rate
    }
    pub fn bitstream_profile_level_id(&self) -> Option<&str> {
        self.bitstream_profile_level_id.as_deref()
    }
    pub fn slots(&self) -> usize {
        self.slots.len()
    }
    pub fn encode(&mut self, index: usize, mut timing: FrameTiming) -> Result<()> {
        if self.eos {
            return Err(EncodeError::SessionFailed);
        }
        let slot = self
            .slots
            .get(index)
            .ok_or(EncodeError::InvalidConfig("slot inválido"))?;
        if self.pending.len() >= self.slots.len() {
            return Err(EncodeError::Busy);
        }
        let frame = self
            .session
            .frames
            .iter()
            .copied()
            .find(|&frame| {
                // SAFETY: frame alocado pela API, retenções do codec são contadas pelo AVBuffer.
                unsafe { (self.session.api.frame_writable)(frame) > 0 }
            })
            .ok_or(EncodeError::Busy)?;
        timing.qpc_encode_start = qpc()?;
        let mut stage_started = self.config.readback_metrics.then(Instant::now);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: textura de origem/staging no mesmo device; Map espera só a cópia submetida, sem fila extra de frames.
        unsafe {
            self.gpu.context.CopyResource(&slot.staging, &slot.texture);
            timing.readback_submit_ms = stage_started
                .as_ref()
                .map(|start| start.elapsed().as_secs_f64() * 1000.);
            stage_started = stage_started.map(|_| Instant::now());
            self.gpu
                .context
                .Map(&slot.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            timing.readback_map_ms = stage_started
                .as_ref()
                .map(|start| start.elapsed().as_secs_f64() * 1000.);
            stage_started = stage_started.map(|_| Instant::now());
            let copied = if self.session.i420 {
                convert_to_i420(&self.config, &mapped, frame)
            } else {
                for (plane, rows, source_offset) in [
                    (0, self.config.height, 0),
                    (
                        1,
                        self.config.height / 2,
                        self.config.height as usize * mapped.RowPitch as usize,
                    ),
                ] {
                    for row in 0..rows as usize {
                        ptr::copy_nonoverlapping(
                            mapped
                                .pData
                                .cast::<u8>()
                                .add(source_offset + row * mapped.RowPitch as usize),
                            (*frame).data[plane].add(row * (*frame).linesize[plane] as usize),
                            self.config.width as usize,
                        );
                    }
                }
                Ok(())
            };
            // Unmap sempre, inclusive quando a conversão recusa o mapeamento.
            self.gpu.context.Unmap(&slot.staging, 0);
            copied?;
            timing.readback_copy_ms = stage_started
                .as_ref()
                .map(|start| start.elapsed().as_secs_f64() * 1000.);
            (*frame).pts = self.index as i64;
            (*frame).pict_type = if self.keyframe { 1 } else { 0 };
        }
        stage_started = stage_started.map(|_| Instant::now());
        // SAFETY: frame de entrada e contexto vivos; API retém referências se necessário.
        let status = unsafe { (self.session.api.send)(self.session.context, frame) };
        timing.codec_send_ms = stage_started
            .as_ref()
            .map(|start| start.elapsed().as_secs_f64() * 1000.);
        if status == -11 {
            return Err(EncodeError::Busy);
        }
        if status < 0 && self.rate_unverified {
            // O driver recusou o Reset da taxa: volta à reabertura, já com a taxa nova, e
            // devolve Busy para o chamador reenviar este mesmo slot na sessão nova.
            let error = self.session.api.check(status, "avcodec_send_frame").err();
            tracing::warn!(
                encoder = self.kind.name(),
                ?error,
                "Troca de bitrate na sessão aberta falhou; usando reabertura do encoder"
            );
            self.inplace_rate = false;
            self.rate_unverified = false;
            if !self.pending.is_empty() {
                return Err(EncodeError::SessionFailed);
            }
            self.session = Session::new(
                self.session.api.clone(),
                &self.config,
                self.kind,
                self.slots.len(),
            )?;
            self.keyframe = true;
            return Err(EncodeError::Busy);
        }
        self.session.api.check(status, "avcodec_send_frame")?;
        self.rate_unverified = false;
        self.keyframe = false;
        timing.qpc_encode_return = qpc()?;
        self.pending.push_back(Pending {
            timing,
            index: self.index,
        })?;
        self.index += 1;
        Ok(())
    }
    pub fn poll_into(&mut self, output: &mut AuBuffer, wait: bool) -> Result<bool> {
        if self.pending.is_empty() {
            return Ok(false);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            // SAFETY: contexto/packet vivos, uso exclusivo.
            let status =
                unsafe { (self.session.api.receive)(self.session.context, self.session.packet) };
            if status == -11 {
                if !wait {
                    return Ok(false);
                }
                if Instant::now() >= deadline {
                    return Err(unavailable("timeout de saída do encoder"));
                }
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            self.session.api.check(status, "avcodec_receive_packet")?;
            // SAFETY: receive preencheu packet; data válido até unref, inclusive em erro de capacidade.
            let result = unsafe {
                let packet = &*self.session.packet;
                if packet.size <= 0 || packet.data.is_null() {
                    Err(unavailable("packet vazio"))
                } else {
                    output.copy_from(std::slice::from_raw_parts(
                        packet.data,
                        packet.size as usize,
                    ))
                }
            };
            // SAFETY: packet alocado pela API, pode ser reutilizado após unref.
            unsafe {
                (self.session.api.packet_unref)(self.session.packet);
            }
            result?;
            let pending = self.pending.pop_front().ok_or(EncodeError::SessionFailed)?;
            output.timing = pending.timing;
            output.timing.qpc_encode_end = qpc()?;
            output.index = pending.index;
            output.kind = if crate::bitstream::inspect_annex_b(VideoCodec::H264, &output.data).key {
                FrameKind::Key
            } else {
                FrameKind::Delta
            };
            return Ok(true);
        }
    }
    pub fn end_of_stream(&mut self) -> Result<()> {
        if !self.eos {
            // SAFETY: nulo sinaliza EOS; contexto vivo.
            let status = unsafe { (self.session.api.send)(self.session.context, ptr::null()) };
            if status == -11 {
                return Err(EncodeError::Busy);
            }
            self.session.api.check(status, "avcodec EOS")?;
            self.eos = true;
        }
        Ok(())
    }
    pub fn reconfigure_rate(&mut self, bitrate: u32, fps: FrameRate) -> Result<()> {
        let mut config = self.config.clone();
        config.bitrate_bps = bitrate;
        config.fps = fps;
        config.validate()?;
        config.clamp_h264_bitrate();
        if config.bitrate_bps == self.config.bitrate_bps && fps == self.config.fps {
            return Ok(());
        }
        if self.inplace_rate && fps == self.config.fps {
            // QuickSync: sem quadros em voo, a recuperação de um Reset recusado pode reabrir a
            // sessão sem perder saída (o próximo send é o primeiro após a troca).
            if self.kind == BackendKind::Qsv && !self.pending.is_empty() {
                return Err(EncodeError::Busy);
            }
            // libx264.c reconfig_encoder() e qsvenc.c update_bitrate() (FFmpeg 760d10603e) leem
            // bit_rate/rc_max_rate/rc_buffer_size a cada quadro: sem nova sessão e sem IDR forçado.
            let vbv = config.vbv_bits()?.to_string();
            let bitrate = config.bitrate_bps.to_string();
            for (key, value) in [("b", &bitrate), ("maxrate", &bitrate), ("bufsize", &vbv)] {
                self.session.api.set(self.session.context, key, value)?;
            }
            self.rate_unverified = self.kind == BackendKind::Qsv;
            self.config = config;
            return Ok(());
        }
        if !self.pending.is_empty() {
            return Err(EncodeError::Busy);
        }
        // Reabertura apenas entre frames: aplica a taxa de fato em todos os drivers e inicia novo IDR.
        let next = Session::new(
            self.session.api.clone(),
            &config,
            self.kind,
            self.slots.len(),
        )?;
        self.session = next;
        self.config = config;
        self.keyframe = true;
        Ok(())
    }
    pub fn request_keyframe(&mut self) {
        self.keyframe = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requer runtime FFmpeg, Windows MF e sessao COM; experimento CPU sem GPU"]
    fn runtime_mf_single_frame_contract() -> Result<()> {
        let _runtime = crate::platform::Runtime::new()?;
        let api = Api::load()?;
        let config = EncoderConfig {
            width: 1280,
            height: 720,
            fps: FrameRate::new(30, 1)?,
            bitrate_bps: 4_000_000,
            h264_level: Some(31),
            ..Default::default()
        };
        let mut session = Session::new(api, &config, BackendKind::Mf, 3)?;
        let actual = session.smoke(&config)?;
        assert!(actual.is_some());
        eprintln!("MF single-frame SPS profile: {actual:?}");
        Ok(())
    }

    #[test]
    #[ignore = "CPU45: requer candidato FFmpeg com libopenh264 em MADSCREEN_ENCODER_RUNTIME; sem GPU"]
    fn runtime_openh264_i420_idr_and_reopen_contract() -> Result<()> {
        use crate::config::H264Profile;
        let _runtime = crate::platform::Runtime::new()?;
        let api = Api::load()?;
        for (width, height, fps, profile, expected) in [
            (
                1280u32,
                720u32,
                30u32,
                H264Profile::ConstrainedBaseline,
                "42",
            ),
            (1280, 720, 30, H264Profile::High, "64"),
            (1920, 1080, 60, H264Profile::ConstrainedBaseline, "42"),
            (1920, 1080, 60, H264Profile::High, "64"),
        ] {
            let config = EncoderConfig {
                width,
                height,
                fps: FrameRate::new(fps, 1)?,
                h264_profile: profile,
                bitrate_bps: 18_000_000,
                ..Default::default()
            };
            config.validate()?;
            // Dois ciclos de abertura: o segundo e a reabertura entre quadros usada pelo feedback de taxa.
            for cycle in 0..2 {
                let opened = Instant::now();
                let mut session = Session::new(api.clone(), &config, BackendKind::OpenH264, 3)?;
                let open_ms = opened.elapsed().as_secs_f64() * 1000.0;
                let smoke = session.smoke(&config)?;
                eprintln!(
                    "CPU45 open cycle={cycle} {width}x{height}@{fps} profile={profile:?} open_ms={open_ms:.1} smoke_profile_level_id={smoke:?}"
                );
                let (w, h) = (width as usize, height as usize);
                // NV12 sintetico com pitch maior que a largura, convertido pelo caminho real para os planos do FFmpeg.
                let pitch = w + 64;
                let mut nv12 = vec![0u8; pitch * h * 3 / 2];
                for (row, line) in nv12.chunks_mut(pitch).take(h).enumerate() {
                    for (column, byte) in line.iter_mut().take(w).enumerate() {
                        *byte = (16 + (row + column) % 200) as u8;
                    }
                }
                for line in nv12[pitch * h..].chunks_mut(pitch) {
                    for (column, byte) in line.iter_mut().take(w).enumerate() {
                        *byte = if column % 2 == 0 { 100 } else { 150 };
                    }
                }
                for index in 0..8i64 {
                    let frame = session.frames[index as usize % session.frames.len()];
                    let requested_idr = index == 0 || index == 3 || index == 7;
                    let started = Instant::now();
                    // SAFETY: sessao exclusiva; quadro YUV420P de av_frame_get_buffer, strides/dimensoes validados abaixo.
                    unsafe {
                        assert!((api.frame_writable)(frame) > 0);
                        let strides = [0, 1, 2].map(|plane| (*frame).linesize[plane] as usize);
                        let mut planes = I420Planes {
                            y: std::slice::from_raw_parts_mut((*frame).data[0], strides[0] * h),
                            y_stride: strides[0],
                            u: std::slice::from_raw_parts_mut(
                                (*frame).data[1],
                                strides[1] * (h / 2),
                            ),
                            u_stride: strides[1],
                            v: std::slice::from_raw_parts_mut(
                                (*frame).data[2],
                                strides[2] * (h / 2),
                            ),
                            v_stride: strides[2],
                        };
                        nv12_to_i420(&nv12, pitch, w, h, &mut planes)?;
                        (*frame).pts = index;
                        (*frame).pict_type = if requested_idr { 1 } else { 0 };
                        api.check((api.send)(session.context, frame), "CPU45 send")?;
                        loop {
                            let status = (api.receive)(session.context, session.packet);
                            if status == -11 && started.elapsed() < Duration::from_secs(2) {
                                std::thread::sleep(Duration::from_millis(1));
                                continue;
                            }
                            api.check(status, "CPU45 receive sem proximo quadro/flush")?;
                            break;
                        }
                        let packet = &*session.packet;
                        assert!(packet.size > 0 && !packet.data.is_null());
                        assert_eq!(packet.pts, index);
                        let data = std::slice::from_raw_parts(packet.data, packet.size as usize);
                        let info = crate::bitstream::inspect_annex_b(VideoCodec::H264, data);
                        let actual = crate::bitstream::h264_access_unit_profile_level_id(data);
                        eprintln!(
                            "CPU45 frame cycle={cycle} {width}x{height}@{fps} {profile:?} index={index} requested_idr={requested_idr} idr={} sps={} pps={} actual={} bytes={} ms={:.2}",
                            info.key,
                            info.sps,
                            info.pps,
                            actual.as_deref().unwrap_or("none"),
                            packet.size,
                            started.elapsed().as_secs_f64() * 1000.0
                        );
                        assert_eq!(info.key, requested_idr, "IDR so quando pedido");
                        if requested_idr {
                            assert!(info.sps && info.pps, "IDR de recuperacao sem SPS/PPS");
                        }
                        if let Some(actual) = actual {
                            assert!(actual.starts_with(expected), "perfil inesperado: {actual}");
                        }
                        (api.packet_unref)(session.packet);
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "MF45: requires patched isolated MF runtime; no GPU or second-frame/flush escape"]
    fn runtime_mf_recovery_rate_profiles_contract() -> Result<()> {
        use crate::config::H264Profile;
        use std::io::Write;
        let _runtime = crate::platform::Runtime::new()?;
        let api = Api::load()?;
        let output = std::env::var_os("MF45_OUTPUT").map(std::path::PathBuf::from);
        let kind = match std::env::var("MF45_BACKEND").as_deref() {
            Ok("x264") => BackendKind::X264,
            Ok("mf") | Err(_) => BackendKind::Mf,
            _ => return Err(unavailable("MF45_BACKEND must be mf or x264")),
        };
        let cases = [
            (1280, 720, 30, 31, 4_000_000, H264Profile::High),
            (1280, 720, 60, 32, 6_000_000, H264Profile::High),
            (1920, 1080, 30, 40, 8_000_000, H264Profile::High),
            (1920, 1080, 60, 42, 12_000_000, H264Profile::High),
            (1280, 720, 30, 31, 4_000_000, H264Profile::Main),
            (
                1280,
                720,
                30,
                31,
                4_000_000,
                H264Profile::ConstrainedBaseline,
            ),
        ];
        for (width, height, fps, level, bitrate, profile) in cases {
            let config = EncoderConfig {
                width,
                height,
                fps: FrameRate::new(fps, 1)?,
                h264_level: Some(level),
                h264_profile: profile,
                bitrate_bps: bitrate,
                ..Default::default()
            };
            config.validate()?;
            let opened = Instant::now();
            // Session::new is the same between-frame reopen mechanism used by rate feedback.
            let session = Session::new(api.clone(), &config, kind, 3)?;
            eprintln!(
                "MF45 open width={width} height={height} fps={fps} bitrate={bitrate} profile={profile:?} open_ms={:.3}",
                opened.elapsed().as_secs_f64() * 1000.0
            );
            let mut artifact = output
                .as_ref()
                .map(|path| {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path.join(format!("{width}x{height}-{fps}-{profile:?}.h264")))
                })
                .transpose()
                .map_err(|error| unavailable(error.to_string()))?;
            for index in 0..8 {
                if index == 2 || index == 5 {
                    std::thread::sleep(Duration::from_millis(100));
                }
                let frame = session.frames[index as usize % session.frames.len()];
                let requested_idr = index == 0 || index == 3 || index == 7;
                // SAFETY: exclusive session; allocated NV12 planes, stride and dimensions checked.
                unsafe {
                    assert!((api.frame_writable)(frame) > 0);
                    for (plane, rows, value) in [(0, height, 16), (1, height / 2, 128)] {
                        for row in 0..rows as usize {
                            ptr::write_bytes(
                                (*frame).data[plane].add(row * (*frame).linesize[plane] as usize),
                                value,
                                width as usize,
                            );
                        }
                    }
                    (*frame).pts = index;
                    (*frame).pict_type = if requested_idr { 1 } else { 0 };
                    let started = Instant::now();
                    api.check((api.send)(session.context, frame), "MF45 single send")?;
                    loop {
                        let status = (api.receive)(session.context, session.packet);
                        if status == -11 && started.elapsed() < Duration::from_secs(2) {
                            std::thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        api.check(status, "MF45 receive before next input/flush")?;
                        break;
                    }
                    let packet = &*session.packet;
                    assert!(packet.size > 0 && !packet.data.is_null());
                    assert_eq!(packet.pts, index);
                    assert_eq!(packet.dts, index);
                    let data = std::slice::from_raw_parts(packet.data, packet.size as usize);
                    let info = crate::bitstream::inspect_annex_b(VideoCodec::H264, data);
                    let actual = crate::bitstream::h264_access_unit_profile_level_id(data);
                    eprintln!(
                        "MF45 frame width={width} height={height} fps={fps} profile={profile:?} index={index} requested_idr={requested_idr} idr={} sps={} pps={} actual={} bytes={} latency_ms={:.3}",
                        info.key,
                        info.sps,
                        info.pps,
                        actual.as_deref().unwrap_or("none"),
                        packet.size,
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                    if let Some(file) = artifact.as_mut() {
                        file.write_all(data)
                            .map_err(|error| unavailable(error.to_string()))?;
                    }
                    assert_eq!(
                        info.key, requested_idr,
                        "MF45 requested recovery must be real IDR"
                    );
                    if requested_idr {
                        assert!(
                            info.sps && info.pps,
                            "MF45 late join requires SPS/PPS at every recovery IDR"
                        );
                    }
                    if let Some(actual) = actual {
                        let expected = match profile {
                            H264Profile::High => "64",
                            H264Profile::Main => "4d",
                            H264Profile::ConstrainedBaseline => "42",
                        };
                        assert!(
                            actual.starts_with(expected),
                            "MF45 profile mismatch: {actual}"
                        );
                        assert_eq!(u32::from_str_radix(&actual[4..], 16).ok(), Some(level));
                        if profile == H264Profile::ConstrainedBaseline {
                            assert_ne!(
                                u8::from_str_radix(&actual[2..4], 16).ok().unwrap_or(0) & 0x40,
                                0,
                                "MF45 constrained baseline flag missing"
                            );
                        }
                    }
                    (api.packet_unref)(session.packet);
                }
            }
            // SAFETY: all eight inputs already produced output; EOS is checked only afterward.
            unsafe {
                api.check(
                    (api.send)(session.context, ptr::null()),
                    "MF45 EOS after complete output",
                )?;
                let status = (api.receive)(session.context, session.packet);
                assert_eq!(
                    status, -541478725,
                    "MF45 flush must have no hidden pending frame"
                );
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requer runtime FFmpeg em MADSCREEN_ENCODER_RUNTIME; não exige GPU AMD"]
    fn runtime_amf_options_allow_single_frame_and_recovery() -> Result<()> {
        let api = Api::load()?;
        // Testa as opções reais do runtime distribuído, antes de abrir o driver.
        // SAFETY: símbolo oficial, nome NUL; retorno verificado antes de alocar.
        let codec = unsafe { (api.find)(c"h264_amf".as_ptr()) };
        assert!(!codec.is_null(), "runtime precisa incluir h264_amf");
        let session = Session {
            // SAFETY: codec não nulo, da API retida pela Session.
            context: unsafe { (api.alloc)(codec) },
            packet: ptr::null_mut(),
            frames: Vec::new(),
            i420: false,
            api: api.clone(),
        };
        assert!(!session.context.is_null());
        type GetInt = unsafe extern "C" fn(*mut c_void, *const c_char, i32, *mut i64) -> i32;
        // SAFETY: assinatura oficial av_opt_get_int, DLL retida em api.
        let get = unsafe { api._util.symbol::<GetInt>(s!("av_opt_get_int"))? };
        let read = |key: &CStr| -> Result<i64> {
            let mut value = 0;
            // SAFETY: contexto vivo, nome NUL e saída i64 válida.
            api.check(
                unsafe { get(session.context, key.as_ptr(), 1, &mut value) },
                "get option",
            )?;
            Ok(value)
        };
        const LOW_DELAY: i64 = 1 << 19;
        assert_eq!(read(c"flags")? & LOW_DELAY, 0);
        assert_eq!(read(c"forced_idr")?, 0);
        configure_amf(&api, session.context)?;
        assert_ne!(read(c"flags")? & LOW_DELAY, 0);
        assert_eq!(read(c"forced_idr")?, 1);
        assert_eq!(read(c"async_depth")?, 1);
        Ok(())
    }

    #[test]
    fn public_prefix_offsets_match_ffmpeg_8_windows_x64() {
        assert_eq!(std::mem::offset_of!(Frame, width), 104);
        assert_eq!(std::mem::offset_of!(Frame, pict_type), 120);
        assert_eq!(std::mem::offset_of!(Frame, pts), 136);
        assert_eq!(std::mem::offset_of!(Frame, duration), 408);
        assert_eq!(std::mem::offset_of!(Packet, data), 24);
        assert_eq!(std::mem::offset_of!(Packet, flags), 40);
    }

    #[test]
    #[ignore = "requer runtime FFmpeg em MADSCREEN_ENCODER_RUNTIME; não exige GPU"]
    fn runtime_cpu_fallback_keyframe_and_rate() -> Result<()> {
        let api = Api::load()?;
        let config = EncoderConfig {
            width: 320,
            height: 180,
            fps: FrameRate::new(60, 1)?,
            bitrate_bps: 2_000_000,
            h264_level: Some(31),
            ..Default::default()
        };
        // Testa falha real seguida de CPU, sem depender de placa/driver no runner.
        let (kind, mut session) =
            crate::backend::select([BackendKind::Nvenc, BackendKind::X264], |kind| {
                let mut session = Session::new(api.clone(), &config, kind, 3)?;
                session.smoke(&config)?;
                Session::new(api.clone(), &config, kind, 3)
            })
            .map_err(|_| unavailable("fallback CPU não inicializou"))?;
        assert_eq!(kind, BackendKind::X264);
        let frame = session.frames[0];
        // Inicializa os planos uma vez; o encoder zerolatency libera a entrada após receber saída.
        // SAFETY: planos NV12 alocados na Session, dimensões e strides válidos.
        unsafe {
            for (plane, rows, value) in [(0, config.height, 16), (1, config.height / 2, 128)] {
                for row in 0..rows as usize {
                    ptr::write_bytes(
                        (*frame).data[plane].add(row * (*frame).linesize[plane] as usize),
                        value,
                        config.width as usize,
                    );
                }
            }
        }
        for index in 0..8 {
            // SAFETY: contexto/frame/packet exclusivos, sem B-frames ou lookahead.
            unsafe {
                assert!((api.frame_writable)(frame) > 0);
                (*frame).pts = index;
                (*frame).pict_type = if index == 0 || index == 4 { 1 } else { 0 };
                api.check((api.send)(session.context, frame), "test send")?;
                api.check(
                    (api.receive)(session.context, session.packet),
                    "test receive",
                )?;
                let packet = &*session.packet;
                assert_eq!(packet.pts, index);
                assert!(packet.size > 0);
                let info = crate::bitstream::inspect_annex_b(
                    VideoCodec::H264,
                    std::slice::from_raw_parts(packet.data, packet.size as usize),
                );
                assert_eq!(info.key, index == 0 || index == 4);
                if info.key {
                    assert!(info.sps && info.pps);
                }
                (api.packet_unref)(session.packet);
            }
        }
        // Mesmo mecanismo de reabertura usado no feedback de rede, com outra taxa/FPS.
        let next = EncoderConfig {
            bitrate_bps: 1_000_000,
            fps: FrameRate::new(30, 1)?,
            ..config
        };
        session = Session::new(api, &next, kind, 3)?;
        session.smoke(&next)?;
        Ok(())
    }

    #[test]
    #[ignore = "requer runtime FFmpeg em MADSCREEN_ENCODER_RUNTIME; não exige GPU"]
    fn runtime_x264_rate_changes_inside_open_session() -> Result<()> {
        let api = Api::load()?;
        let mut config = EncoderConfig {
            width: 640,
            height: 360,
            fps: FrameRate::new(30, 1)?,
            bitrate_bps: 6_000_000,
            h264_level: Some(31),
            ..Default::default()
        };
        let session = Session::new(api.clone(), &config, BackendKind::X264, 1)?;
        let frame = session.frames[0];
        let mut seed = 0x9e37_79b9_u32;
        let mut encode = |index: i64| -> Result<(usize, bool)> {
            // SAFETY: frame/contexto/packet exclusivos da Session; planos NV12 alocados nela.
            unsafe {
                for (plane, rows) in [(0, config.height), (1, config.height / 2)] {
                    for row in 0..rows as usize {
                        let line =
                            (*frame).data[plane].add(row * (*frame).linesize[plane] as usize);
                        for column in 0..config.width as usize {
                            seed ^= seed << 13;
                            seed ^= seed >> 17;
                            seed ^= seed << 5;
                            *line.add(column) = seed as u8;
                        }
                    }
                }
                (*frame).pts = index;
                (*frame).pict_type = i32::from(index == 0);
                api.check((api.send)(session.context, frame), "send")?;
                api.check((api.receive)(session.context, session.packet), "receive")?;
                let packet = &*session.packet;
                let info = crate::bitstream::inspect_annex_b(
                    VideoCodec::H264,
                    std::slice::from_raw_parts(packet.data, packet.size as usize),
                );
                let size = packet.size as usize;
                (api.packet_unref)(session.packet);
                Ok((size, info.key))
            }
        };
        let high: usize = (0..30)
            .map(|i| encode(i).map(|(s, _)| s))
            .sum::<Result<_>>()?;
        config.bitrate_bps = 500_000;
        let vbv = config.vbv_bits()?.to_string();
        for (key, value) in [
            ("b", "500000"),
            ("maxrate", "500000"),
            ("bufsize", vbv.as_str()),
        ] {
            api.set(session.context, key, value)?;
        }
        let mut low = 0;
        for index in 30..90 {
            let (size, key) = encode(index)?;
            assert!(
                !key,
                "troca de taxa na sessão não pode gerar IDR (quadro {index})"
            );
            if index >= 60 {
                low += size;
            }
        }
        // 30 quadros a 6 Mbps vs. 30 quadros após assentar em 0,5 Mbps.
        assert!(low * 4 < high, "taxa não aplicada: alto={high} baixo={low}");
        Ok(())
    }
}
