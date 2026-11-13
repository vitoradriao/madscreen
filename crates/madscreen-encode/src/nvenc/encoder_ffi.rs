#![allow(unsafe_code)]
use crate::pipeline::PendingQueue;
use crate::platform::FrameEvent;
use crate::{
    AuBuffer, EncodeError, Encoder, EncoderConfig, FrameKind, FrameRate, FrameTiming, Result,
    VideoCodec,
    nvenc_sys::*,
    parameters_ffi::{check, initialize_driver_mode, lock_bitstream, reconfigure_driver_mode},
    platform::{dll_ffi::Library, qpc, qpc_frequency},
    versions::*,
};
use madscreen_capture::wgc::GpuDevice;
use std::{ffi::c_void, ptr};
use windows::{
    Win32::Graphics::{
        Direct3D11::{D3D11_TEXTURE2D_DESC, ID3D11Texture2D},
        Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12},
    },
    core::{Interface, s, w},
};
fn required<T>(f: Option<T>) -> Result<T> {
    f.ok_or_else(|| {
        EncodeError::EncoderUnavailable(
            "função NVENC obrigatória ausente na tabela do driver".into(),
        )
    })
}

struct Api {
    functions: NV_ENCODE_API_FUNCTION_LIST,
    max_version: u32,
    _library: Library,
}
impl Api {
    fn new() -> Result<Self> {
        let library = Library::load(w!("nvEncodeAPI64.dll")).map_err(|e| {
            EncodeError::EncoderUnavailable(format!(
                "nvEncodeAPI64.dll ausente ou inválida; instale o driver NVIDIA: {e}"
            ))
        })?;
        type GetVersion = unsafe extern "C" fn(*mut u32) -> NVENCSTATUS;
        type Create = unsafe extern "C" fn(*mut NV_ENCODE_API_FUNCTION_LIST) -> NVENCSTATUS;
        // SAFETY: assinaturas do header oficial; DLL permanece viva em Api.
        let (get_version, create) = unsafe {
            (
                library.symbol::<GetVersion>(s!("NvEncodeAPIGetMaxSupportedVersion"))?,
                library.symbol::<Create>(s!("NvEncodeAPICreateInstance"))?,
            )
        };
        let mut max_version = 0;
        // SAFETY: saída local de 32 bits válida.
        check("NvEncodeAPIGetMaxSupportedVersion", unsafe {
            get_version(&mut max_version)
        })?;
        check_driver_version(max_version)?;
        let mut functions = NV_ENCODE_API_FUNCTION_LIST {
            version: NV_ENCODE_API_FUNCTION_LIST_VER,
            ..Default::default()
        };
        // SAFETY: tabela zerada com versão exata do header 12.2.
        check("NvEncodeAPICreateInstance", unsafe {
            create(&mut functions)
        })?;
        // Validar todo o ciclo antes de adquirir qualquer recurso.
        required(functions.nvEncOpenEncodeSessionEx)?;
        required(functions.nvEncDestroyEncoder)?;
        required(functions.nvEncGetEncodeGUIDCount)?;
        required(functions.nvEncGetEncodeGUIDs)?;
        required(functions.nvEncGetEncodeCaps)?;
        required(functions.nvEncGetEncodePresetConfigEx)?;
        required(functions.nvEncInitializeEncoder)?;
        required(functions.nvEncRegisterResource)?;
        required(functions.nvEncUnregisterResource)?;
        required(functions.nvEncCreateBitstreamBuffer)?;
        required(functions.nvEncDestroyBitstreamBuffer)?;
        required(functions.nvEncMapInputResource)?;
        required(functions.nvEncUnmapInputResource)?;
        required(functions.nvEncEncodePicture)?;
        required(functions.nvEncLockBitstream)?;
        required(functions.nvEncUnlockBitstream)?;
        required(functions.nvEncReconfigureEncoder)?;
        Ok(Self {
            functions,
            max_version,
            _library: library,
        })
    }
}
struct Session {
    api: Api,
    handle: *mut c_void,
    _gpu: GpuDevice,
}
impl Session {
    fn open(gpu: &GpuDevice) -> Result<Self> {
        let api = Api::new()?;
        let mut params = NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS {
            version: NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER,
            deviceType: NV_ENC_DEVICE_TYPE_DIRECTX,
            device: gpu.device.as_raw(),
            apiVersion: NVENCAPI_VERSION,
            ..Default::default()
        };
        let open = required(api.functions.nvEncOpenEncodeSessionEx)?;
        let mut session = Self {
            api,
            handle: ptr::null_mut(),
            _gpu: gpu.clone(),
        };
        // SAFETY: o mesmo ID3D11Device da captura fica retido na sessão até DestroyEncoder.
        check("NvEncOpenEncodeSessionEx", unsafe {
            open(&mut params, &mut session.handle)
        })?;
        if session.handle.is_null() {
            return Err(EncodeError::EncoderUnavailable(
                "driver retornou sessão nula".into(),
            ));
        }
        Ok(session)
    }
    fn cap(&self, codec: GUID, cap: NV_ENC_CAPS) -> Result<i32> {
        let mut params = NV_ENC_CAPS_PARAM {
            version: NV_ENC_CAPS_PARAM_VER,
            capsToQuery: cap,
            ..Default::default()
        };
        let mut value = 0;
        // SAFETY: sessão e ponteiros de saída válidos, struct zerada/versionada.
        check("NvEncGetEncodeCaps", unsafe {
            required(self.api.functions.nvEncGetEncodeCaps)?(
                self.handle,
                codec,
                &mut params,
                &mut value,
            )
        })?;
        Ok(value)
    }
    fn cap_or_zero(&self, codec: GUID, cap: NV_ENC_CAPS) -> i32 {
        self.cap(codec, cap).unwrap_or(0)
    }
    fn codecs(&self) -> Result<Vec<CodecCaps>> {
        let mut count = 0;
        // SAFETY: sessão viva e contador de saída válido.
        check("NvEncGetEncodeGUIDCount", unsafe {
            required(self.api.functions.nvEncGetEncodeGUIDCount)?(self.handle, &mut count)
        })?;
        if count > 64 {
            return Err(EncodeError::EncoderUnavailable(
                "quantidade inesperada de codecs do driver".into(),
            ));
        }
        let mut guids = vec![GUID::default(); count as usize];
        let mut actual = 0;
        // SAFETY: vetor dimensionado pela contagem retornada, disponível só na inicialização.
        check("NvEncGetEncodeGUIDs", unsafe {
            required(self.api.functions.nvEncGetEncodeGUIDs)?(
                self.handle,
                guids.as_mut_ptr(),
                count,
                &mut actual,
            )
        })?;
        let mut result = Vec::new();
        for codec in [VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1] {
            let guid = codec.guid();
            if guids[..(actual as usize).min(guids.len())]
                .iter()
                .any(|g| same_guid(*g, guid))
            {
                // NvEncGetEncodeCaps consulta em runtime o limite real por GPU/codec; H.264 costuma ser 4096.
                result.push(CodecCaps {
                    codec,
                    min_width: self.cap_or_zero(guid, NV_ENC_CAPS_WIDTH_MIN),
                    min_height: self.cap_or_zero(guid, NV_ENC_CAPS_HEIGHT_MIN),
                    max_width: self.cap(guid, NV_ENC_CAPS_WIDTH_MAX)?,
                    max_height: self.cap(guid, NV_ENC_CAPS_HEIGHT_MAX)?,
                    level_max: self.cap_or_zero(guid, NV_ENC_CAPS_LEVEL_MAX),
                    mb_num_max: self.cap_or_zero(guid, NV_ENC_CAPS_MB_NUM_MAX),
                    mb_per_sec_max: self.cap_or_zero(guid, NV_ENC_CAPS_MB_PER_SEC_MAX),
                    asynchronous: self.cap(guid, NV_ENC_CAPS_ASYNC_ENCODE_SUPPORT)? != 0,
                    intra_refresh: self.cap(guid, NV_ENC_CAPS_SUPPORT_INTRA_REFRESH)? != 0,
                    dynamic_bitrate: self.cap(guid, NV_ENC_CAPS_SUPPORT_DYN_BITRATE_CHANGE)? != 0,
                });
            }
        }
        Ok(result)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            if let Some(destroy) = self.api.functions.nvEncDestroyEncoder {
                // SAFETY: dono único da sessão; recursos externos já foram removidos pelo NvencEncoder.
                let status = unsafe { destroy(self.handle) };
                if let Err(e) = check("NvEncDestroyEncoder", status) {
                    tracing::warn!(%e);
                }
            }
            self.handle = ptr::null_mut();
        }
    }
}
fn same_guid(a: GUID, b: GUID) -> bool {
    a.Data1 == b.Data1 && a.Data2 == b.Data2 && a.Data3 == b.Data3 && a.Data4 == b.Data4
}
#[derive(Debug, serde::Serialize)]
pub struct CodecCaps {
    pub codec: VideoCodec,
    pub min_width: i32,
    pub min_height: i32,
    pub max_width: i32,
    pub max_height: i32,
    pub level_max: i32,
    pub mb_num_max: i32,
    pub mb_per_sec_max: i32,
    pub asynchronous: bool,
    pub intra_refresh: bool,
    pub dynamic_bitrate: bool,
}
#[derive(Debug, serde::Serialize)]
pub struct NvencProbe {
    pub api_used: &'static str,
    pub max_api_major: u32,
    pub max_api_minor: u32,
    pub codecs: Vec<CodecCaps>,
}
pub fn probe(gpu: &GpuDevice) -> Result<NvencProbe> {
    let session = Session::open(gpu)?;
    Ok(NvencProbe {
        api_used: "12.2",
        max_api_major: session.api.max_version >> 4,
        max_api_minor: session.api.max_version & 15,
        codecs: session.codecs()?,
    })
}
#[derive(Clone)]
pub struct Nv12Surface {
    index: usize,
    texture: ID3D11Texture2D,
}
impl Nv12Surface {
    pub fn texture(&self) -> &ID3D11Texture2D {
        &self.texture
    }
    pub fn index(&self) -> usize {
        self.index
    }
}
struct Slot {
    surface: Nv12Surface,
    registered: NV_ENC_REGISTERED_PTR,
    bitstream: NV_ENC_OUTPUT_PTR,
    mapped: NV_ENC_INPUT_PTR,
    locked: bool,
    event: Option<std::sync::Arc<FrameEvent>>,
}
struct Pending {
    slot: usize,
    timing: FrameTiming,
    index: u64,
}
pub struct NvencEncoder {
    session: Session,
    config: EncoderConfig,
    native: NV_ENC_CONFIG,
    slots: Vec<Slot>,
    next: usize,
    pending: PendingQueue<Pending>,
    requested_key: bool,
    last_key: i64,
    frequency: i64,
    index: u64,
    input_format: NV_ENC_BUFFER_FORMAT,
    failed: bool,
    eos_sent: bool,
}
// SAFETY: a sessão é exclusiva, todas as operações exigem &mut; device/context são multithread protected.
// A DLL e os recursos permanecem vivos ao mover a sessão para outra thread.
unsafe impl Send for NvencEncoder {}
impl NvencEncoder {
    pub fn new(
        gpu: &GpuDevice,
        mut config: EncoderConfig,
        textures: &[ID3D11Texture2D],
        bgra: bool,
    ) -> Result<Self> {
        config.validate()?;
        config.clamp_h264_bitrate();
        if !(2..=16).contains(&textures.len()) {
            return Err(EncodeError::InvalidConfig("anel exige 2–16 texturas"));
        }
        let session = Session::open(gpu)?;
        let caps = session.codecs()?;
        let supported = caps
            .iter()
            .find(|c| c.codec == config.codec)
            .ok_or_else(|| {
                EncodeError::EncoderUnavailable(format!(
                    "codec {:?} ausente nesta GPU; use H.264/HEVC",
                    config.codec
                ))
            })?;
        if config.codec == VideoCodec::H264
            && supported.level_max > 0
            && config
                .h264_level
                .is_some_and(|level| i64::from(level) > i64::from(supported.level_max))
        {
            return Err(EncodeError::InvalidConfig(
                "nível H.264 acima do suportado pela GPU",
            ));
        }
        if i64::from(config.width) > i64::from(supported.max_width)
            || i64::from(config.height) > i64::from(supported.max_height)
        {
            return Err(EncodeError::InvalidConfig(
                "resolução excede os limites do codec nesta GPU",
            ));
        }
        if config.intra_refresh.is_some() && !supported.intra_refresh {
            return Err(EncodeError::InvalidConfig(
                "intra-refresh não suportado pelo codec nesta GPU",
            ));
        }
        if !supported.asynchronous {
            return Err(EncodeError::EncoderUnavailable(
                "GPU não oferece eventos NVENC assíncronos".into(),
            ));
        }
        // SAFETY: sessão aberta por esta tabela, ainda não inicializada; API e device retidos.
        let native = unsafe {
            initialize_driver_mode(&session.api.functions, session.handle, &config, true)?
        };
        let input_format = if bgra {
            NV_ENC_BUFFER_FORMAT_ARGB
        } else {
            NV_ENC_BUFFER_FORMAT_NV12
        };
        let mut encoder = Self {
            session,
            config,
            native,
            slots: Vec::with_capacity(textures.len()),
            next: 0,
            pending: PendingQueue::new(textures.len()),
            requested_key: true,
            last_key: i64::MIN,
            frequency: qpc_frequency()?,
            index: 0,
            input_format,
            failed: false,
            eos_sent: false,
        };
        for texture in textures {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            // SAFETY: recurso vivo; consulta de metadata, sem Map/readback.
            let owner = unsafe {
                texture.GetDesc(&mut desc);
                texture.GetDevice()?
            };
            if owner.as_raw() != gpu.device.as_raw()
                || desc.Width != encoder.config.width
                || desc.Height != encoder.config.height
                || desc.Format
                    != if bgra {
                        DXGI_FORMAT_B8G8R8A8_UNORM
                    } else {
                        DXGI_FORMAT_NV12
                    }
            {
                return Err(EncodeError::InvalidConfig(
                    "textura deve usar o mesmo device, tamanho e formato da sessão",
                ));
            }
            let index = encoder.slots.len();
            // Adicionar slot antes das chamadas permite limpar inicialização parcial.
            encoder.slots.push(Slot {
                surface: Nv12Surface {
                    index,
                    texture: texture.clone(),
                },
                registered: ptr::null_mut(),
                bitstream: ptr::null_mut(),
                mapped: ptr::null_mut(),
                locked: false,
                event: None,
            });
            let slot = &mut encoder.slots[index];
            let mut register = NV_ENC_REGISTER_RESOURCE {
                version: NV_ENC_REGISTER_RESOURCE_VER,
                resourceType: NV_ENC_INPUT_RESOURCE_TYPE_DIRECTX,
                resourceToRegister: texture.as_raw(),
                width: desc.Width,
                height: desc.Height,
                pitch: 0,
                bufferFormat: input_format,
                bufferUsage: NV_ENC_INPUT_IMAGE,
                ..Default::default()
            };
            // SAFETY: textura retida no slot; pitch=0 para DirectX (driver conhece layout opaco).
            check("NvEncRegisterResource", unsafe {
                required(encoder.session.api.functions.nvEncRegisterResource)?(
                    encoder.session.handle,
                    &mut register,
                )
            })?;
            slot.registered = register.registeredResource;
            let mut bitstream = NV_ENC_CREATE_BITSTREAM_BUFFER {
                version: NV_ENC_CREATE_BITSTREAM_BUFFER_VER,
                ..Default::default()
            };
            // SAFETY: sessão inicializada e descriptor versionado.
            check("NvEncCreateBitstreamBuffer", unsafe {
                required(encoder.session.api.functions.nvEncCreateBitstreamBuffer)?(
                    encoder.session.handle,
                    &mut bitstream,
                )
            })?;
            slot.bitstream = bitstream.bitstreamBuffer;
            let event = std::sync::Arc::new(FrameEvent::manual()?);
            let mut params = NV_ENC_EVENT_PARAMS {
                version: NV_ENC_EVENT_PARAMS_VER,
                completionEvent: event.raw(),
                ..Default::default()
            };
            // SAFETY: evento permanece vivo no slot até UnregisterAsyncEvent e destruição da sessão.
            check("NvEncRegisterAsyncEvent", unsafe {
                required(encoder.session.api.functions.nvEncRegisterAsyncEvent)?(
                    encoder.session.handle,
                    &mut params,
                )
            })?;
            slot.event = Some(event);
        }
        Ok(encoder)
    }
    pub fn surface(&self) -> Result<Nv12Surface> {
        if self.failed {
            return Err(EncodeError::SessionFailed);
        }
        for offset in 0..self.slots.len() {
            let index = (self.next + offset) % self.slots.len();
            if !self.pending.iter().any(|pending| pending.slot == index) {
                return Ok(self.slots[index].surface.clone());
            }
        }
        Err(EncodeError::Busy)
    }
    pub fn surface_at(&self, index: usize) -> Result<Nv12Surface> {
        if self.failed {
            return Err(EncodeError::SessionFailed);
        }
        if self.pending.iter().any(|p| p.slot == index) {
            return Err(EncodeError::Busy);
        }
        self.slots
            .get(index)
            .map(|s| s.surface.clone())
            .ok_or(EncodeError::InvalidConfig("slot inválido"))
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    pub fn end_of_stream(&mut self) -> Result<()> {
        if self.eos_sent {
            return Ok(());
        }
        // Em modo assíncrono o driver recusa o EOS (INVALID_PARAM) sem `completionEvent`: o guia
        // NVENC e o NvEncoder do SDK passam o evento de um slot livre (ainda não submetido).
        let free_event = (0..self.slots.len())
            .map(|offset| (self.next + offset) % self.slots.len())
            .find(|&index| !self.pending.iter().any(|p| p.slot == index))
            .and_then(|index| self.slots[index].event.clone());
        if let Some(event) = &free_event {
            event.reset()?;
        }
        let mut params = NV_ENC_PIC_PARAMS {
            version: NV_ENC_PIC_PARAMS_VER,
            encodePicFlags: NV_ENC_PIC_FLAG_EOS as u32,
            completionEvent: free_event.as_ref().map_or(ptr::null_mut(), |e| e.raw()),
            ..Default::default()
        };
        // SAFETY: sessão viva e exclusiva; EOS após a última submissão, antes de drenar os outputs.
        let result = check("NvEncEncodePicture(EOS)", unsafe {
            required(self.session.api.functions.nvEncEncodePicture)?(
                self.session.handle,
                &mut params,
            )
        });
        // EOS é só um aviso de fim de fluxo: se o driver recusar, ainda dá para drenar os pendentes.
        if let Err(error) = result {
            tracing::warn!(%error, "EOS recusado pelo driver; drenando frames pendentes mesmo assim");
        }
        self.eos_sent = true;
        Ok(())
    }
    fn unmap(&mut self, slot: usize) -> Result<()> {
        let input = self.slots[slot].mapped;
        if input.is_null() {
            return Ok(());
        }
        // SAFETY: recurso foi mapeado por esta sessão e só é desmapeado após output/unlock.
        let status = unsafe {
            required(self.session.api.functions.nvEncUnmapInputResource)?(
                self.session.handle,
                input,
            )
        };
        check("NvEncUnmapInputResource", status)?;
        self.slots[slot].mapped = ptr::null_mut();
        Ok(())
    }
}
impl NvencEncoder {
    fn collect_into(&mut self, output: &mut AuBuffer, nonblocking: bool) -> Result<bool> {
        if self.failed {
            return Err(EncodeError::SessionFailed);
        }
        let Some(front) = self.pending.front() else {
            return Ok(false);
        };
        if let Some(event) = &self.slots[front.slot].event {
            if !event.wait(if nonblocking { 0 } else { 5000 })? {
                return if nonblocking {
                    Ok(false)
                } else {
                    Err(EncodeError::EncoderUnavailable(
                        "timeout no evento NVENC".into(),
                    ))
                };
            }
        }
        let Some(mut pending) = self.pending.pop_front() else {
            return Ok(false);
        };
        let bitstream = self.slots[pending.slot].bitstream;
        let mut lock = NV_ENC_LOCK_BITSTREAM {
            version: NV_ENC_LOCK_BITSTREAM_VER,
            outputBitstream: bitstream,
            ..Default::default()
        };
        // SAFETY: sessão async e evento concluído; output exclusivo do slot pendente.
        let result = unsafe {
            lock_bitstream(
                &self.session.api.functions,
                self.session.handle,
                &mut lock,
                nonblocking,
            )
        };
        match result {
            Ok(true) => {}
            Ok(false) => {
                self.pending.push_front(pending)?;
                return Ok(false);
            }
            Err(error) => {
                self.pending.push_front(pending)?;
                self.failed = true;
                return Err(error);
            }
        }
        self.slots[pending.slot].locked = true;
        let end = qpc();
        let copy = if lock.bitstreamBufferPtr.is_null() || lock.bitstreamSizeInBytes == 0 {
            Err(EncodeError::InvalidBitstream("NVENC retornou AU vazia"))
        } else {
            // SAFETY: buffer da API permanece vivo até Unlock; única leitura GPU→CPU permitida é o bitstream.
            let data = unsafe {
                std::slice::from_raw_parts(
                    lock.bitstreamBufferPtr.cast::<u8>(),
                    lock.bitstreamSizeInBytes as usize,
                )
            };
            output.copy_from(data)
        };
        // SAFETY: pareia exatamente o lock bem-sucedido, inclusive nos caminhos de erro de cópia.
        let unlock = check("NvEncUnlockBitstream", unsafe {
            required(self.session.api.functions.nvEncUnlockBitstream)?(
                self.session.handle,
                bitstream,
            )
        });
        if unlock.is_err() {
            self.failed = true;
        }
        if unlock.is_ok() {
            self.slots[pending.slot].locked = false;
        }
        unlock?;
        let unmap = self.unmap(pending.slot);
        if unmap.is_err() {
            self.failed = true;
        }
        unmap?;
        copy?;
        pending.timing.qpc_encode_end = end?;
        // I simples em H.264/HEVC não implica ponto de acesso aleatório.
        let key = lock.pictureType == NV_ENC_PIC_TYPE_IDR
            || (self.config.codec == VideoCodec::Av1 && lock.pictureType == NV_ENC_PIC_TYPE_I);
        output.kind = if key {
            FrameKind::Key
        } else {
            FrameKind::Delta
        };
        output.timing = pending.timing;
        output.index = pending.index;
        output.qp = Some(lock.frameAvgQP);
        if output.kind == FrameKind::Key {
            crate::bitstream::validate_key_headers(self.config.codec, &output.data)?;
        }
        Ok(true)
    }
}
impl Encoder for NvencEncoder {
    type Surface = Nv12Surface;
    fn encode(&mut self, surface: &Nv12Surface, mut timing: FrameTiming) -> Result<()> {
        if self.failed {
            return Err(EncodeError::SessionFailed);
        }
        if self.eos_sent {
            return Err(EncodeError::InvalidConfig("sessão já recebeu EOS"));
        }
        if self.pending.iter().any(|p| p.slot == surface.index) {
            return Err(EncodeError::Busy);
        }
        if surface.index >= self.slots.len()
            || surface.texture.as_raw() != self.slots[surface.index].surface.texture.as_raw()
        {
            return Err(EncodeError::InvalidConfig(
                "superfície de outra sessão ou slot fora de ordem",
            ));
        }
        self.next = surface.index;
        timing.slot = surface.index;
        let slot = &mut self.slots[self.next];
        let mut mapping = NV_ENC_MAP_INPUT_RESOURCE {
            version: NV_ENC_MAP_INPUT_RESOURCE_VER,
            registeredResource: slot.registered,
            ..Default::default()
        };
        // SAFETY: recurso registrado, Flush submeteu os comandos; modo fence confirma conclusão antes deste ponto.
        let status = unsafe {
            required(self.session.api.functions.nvEncMapInputResource)?(
                self.session.handle,
                &mut mapping,
            )
        };
        if let Err(error) = check("NvEncMapInputResource", status) {
            self.failed = true;
            return Err(error);
        }
        slot.mapped = mapping.mappedResource;
        timing.qpc_encode_start = match qpc() {
            Ok(now) => now,
            Err(e) => {
                self.failed = true;
                return Err(e.into());
            }
        };
        // Alinha a proteção do driver ao limitador de 0,5 s da publicação.
        let force = self.requested_key
            && (self.index == 0
                || timing.qpc_encode_start.saturating_sub(self.last_key) >= self.frequency / 2);
        if let Some(event) = &slot.event {
            event.reset()?;
        }
        let mut picture = NV_ENC_PIC_PARAMS {
            version: NV_ENC_PIC_PARAMS_VER,
            inputWidth: self.config.width,
            inputHeight: self.config.height,
            inputBuffer: slot.mapped,
            outputBitstream: slot.bitstream,
            completionEvent: slot.event.as_ref().map_or(ptr::null_mut(), |e| e.raw()),
            bufferFmt: self.input_format,
            pictureStruct: NV_ENC_PIC_STRUCT_FRAME,
            frameIdx: self.index as u32,
            inputTimeStamp: self.index,
            inputDuration: 1,
            encodePicFlags: if force {
                NV_ENC_PIC_FLAG_FORCEIDR as u32 | NV_ENC_PIC_FLAG_OUTPUT_SPSPPS as u32
            } else {
                0
            },
            ..Default::default()
        };
        timing.qpc_encode_start = qpc()?;
        // SAFETY: input mapeado e output exclusivo do slot, sem B frames/lookahead.
        let status = unsafe {
            required(self.session.api.functions.nvEncEncodePicture)?(
                self.session.handle,
                &mut picture,
            )
        };
        timing.qpc_encode_return = qpc()?;
        if let Err(e) = check("NvEncEncodePicture", status) {
            self.failed = true;
            // NEED_MORE_INPUT não é esperado na política sem reorder. Não reutilizar input até EOS/Destroy.
            if status != NV_ENC_ERR_NEED_MORE_INPUT {
                let _ = self.unmap(self.next);
            }
            return Err(e);
        }
        if force {
            self.requested_key = false;
            self.last_key = timing.qpc_encode_start;
        }
        self.pending.push_back(Pending {
            slot: self.next,
            timing,
            index: self.index,
        })?;
        self.next = (self.next + 1) % self.slots.len();
        self.index += 1;
        Ok(())
    }
    fn poll_into(&mut self, output: &mut AuBuffer) -> Result<bool> {
        self.collect_into(output, false)
    }
    fn try_poll_into(&mut self, output: &mut AuBuffer) -> Result<bool> {
        self.collect_into(output, true)
    }
    fn reconfigure_rate(&mut self, bitrate_bps: u32, fps: FrameRate) -> Result<()> {
        if self.failed {
            return Err(EncodeError::SessionFailed);
        }
        if !self.pending.is_empty() {
            return Err(EncodeError::Busy);
        }
        let mut config = self.config.clone();
        config.bitrate_bps = bitrate_bps;
        config.fps = fps;
        config.validate()?;
        config.clamp_h264_bitrate();
        // SAFETY: mesma sessão/tabela da inicialização; encode pendente foi rejeitado acima.
        let native = unsafe {
            reconfigure_driver_mode(
                &self.session.api.functions,
                self.session.handle,
                &config,
                self.native,
                true,
            )?
        };
        self.config = config;
        self.native = native;
        Ok(())
    }
    fn request_keyframe(&mut self) {
        self.requested_key = true;
    }
}
impl Drop for NvencEncoder {
    fn drop(&mut self) {
        let _ = self.end_of_stream();
        // SAFETY: sessão viva e uso exclusivo. EOS termina inputs aceitos antes de remover recursos.
        unsafe {
            let f = &self.session.api.functions;
            while let Some(pending) = self.pending.pop_front() {
                if let (Some(lock_fn), Some(unlock)) =
                    (f.nvEncLockBitstream, f.nvEncUnlockBitstream)
                {
                    let output = self.slots[pending.slot].bitstream;
                    let mut lock = NV_ENC_LOCK_BITSTREAM {
                        version: NV_ENC_LOCK_BITSTREAM_VER,
                        outputBitstream: output,
                        ..Default::default()
                    };
                    if lock_fn(self.session.handle, &mut lock) == NV_ENC_SUCCESS {
                        let _ = unlock(self.session.handle, output);
                    }
                }
            }
            for slot in &mut self.slots {
                if let Some(event) = &slot.event {
                    if let Some(unregister) = f.nvEncUnregisterAsyncEvent {
                        let mut params = NV_ENC_EVENT_PARAMS {
                            version: NV_ENC_EVENT_PARAMS_VER,
                            completionEvent: event.raw(),
                            ..Default::default()
                        };
                        let _ = unregister(self.session.handle, &mut params);
                    }
                }
                if slot.locked {
                    if let Some(unlock) = f.nvEncUnlockBitstream {
                        let _ = unlock(self.session.handle, slot.bitstream);
                    }
                }

                if !slot.mapped.is_null() {
                    if let Some(unmap) = f.nvEncUnmapInputResource {
                        let _ = unmap(self.session.handle, slot.mapped);
                    }
                }
                if !slot.registered.is_null() {
                    if let Some(unregister) = f.nvEncUnregisterResource {
                        let _ = unregister(self.session.handle, slot.registered);
                    }
                }
                if !slot.bitstream.is_null() {
                    if let Some(destroy) = f.nvEncDestroyBitstreamBuffer {
                        let _ = destroy(self.session.handle, slot.bitstream);
                    }
                }
            }
        }
        // Session::drop destrói o encoder antes de liberar o ID3D11Device e a DLL.
    }
}
