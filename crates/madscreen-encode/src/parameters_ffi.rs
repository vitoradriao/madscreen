//! Montagem ABI pura; preserva os campos retornados pelo preset e altera somente a política.
#![allow(unsafe_code)]
use crate::{EncoderConfig, MultiPass, Result, VideoCodec, nvenc_sys::*, versions::*};

pub fn apply_config(config: &EncoderConfig, native: &mut NV_ENC_CONFIG) -> Result<()> {
    config.validate()?;
    native.version = NV_ENC_CONFIG_VER;
    native.gopLength = NVENC_INFINITE_GOPLENGTH;
    native.frameIntervalP = 1;
    native.frameFieldMode = NV_ENC_PARAMS_FRAME_FIELD_MODE_FRAME;
    let rc = &mut native.rcParams;
    rc.version = NV_ENC_RC_PARAMS_VER;
    rc.rateControlMode = NV_ENC_PARAMS_RC_CBR;
    rc.averageBitRate = config.bitrate_bps;
    rc.maxBitRate = config.bitrate_bps;
    rc.vbvBufferSize = config.vbv_bits()?;
    rc.vbvInitialDelay = rc.vbvBufferSize;
    rc.set_zeroReorderDelay(1);
    rc.set_enableLookahead(0);
    rc.set_enableExtLookahead(0);
    rc.lookaheadDepth = 0;
    rc.set_enableAQ(u32::from(config.spatial_aq));
    rc.set_enableTemporalAQ(0);
    rc.lowDelayKeyFrameScale = 1;
    rc.multiPass = match config.multipass {
        MultiPass::Disabled => NV_ENC_MULTI_PASS_DISABLED,
        MultiPass::QuarterRes => NV_ENC_TWO_PASS_QUARTER_RESOLUTION,
    };
    let (enabled, period, count) = config
        .intra_refresh
        .map_or((0, 0, 0), |i| (1, i.period, i.count));
    let vui = NV_ENC_CONFIG_H264_VUI_PARAMETERS {
        videoSignalTypePresentFlag: 1,
        videoFormat: NV_ENC_VUI_VIDEO_FORMAT_UNSPECIFIED,
        videoFullRangeFlag: 0,
        colourDescriptionPresentFlag: 1,
        colourPrimaries: NV_ENC_VUI_COLOR_PRIMARIES_BT709,
        transferCharacteristics: NV_ENC_VUI_TRANSFER_CHARACTERISTIC_BT709,
        colourMatrix: NV_ENC_VUI_MATRIX_COEFFS_BT709,
        ..Default::default()
    };
    // SAFETY: o preset foi consultado para o mesmo codec; structs C triviais e union com armazenamento suficiente.
    unsafe {
        match config.codec {
            VideoCodec::H264 => {
                native.profileGUID = match config.h264_profile {
                    crate::config::H264Profile::High => NV_ENC_H264_PROFILE_HIGH_GUID,
                    crate::config::H264Profile::Main => NV_ENC_H264_PROFILE_MAIN_GUID,
                    crate::config::H264Profile::ConstrainedBaseline => {
                        NV_ENC_H264_PROFILE_BASELINE_GUID
                    }
                };
                let c = &mut native.encodeCodecConfig.h264Config;
                if config.h264_profile != crate::config::H264Profile::High {
                    c.adaptiveTransformMode = NV_ENC_H264_ADAPTIVE_TRANSFORM_DISABLE;
                }
                if config.h264_profile == crate::config::H264Profile::ConstrainedBaseline {
                    c.entropyCodingMode = NV_ENC_H264_ENTROPY_CODING_MODE_CAVLC;
                }
                c.level = config.h264_level.unwrap_or(NV_ENC_LEVEL_AUTOSELECT as u32);
                c.idrPeriod = NVENC_INFINITE_GOPLENGTH;
                c.set_repeatSPSPPS(1);
                c.set_disableSPSPPS(0);
                c.chromaFormatIDC = 1;
                c.set_enableIntraRefresh(enabled);
                c.intraRefreshPeriod = period;
                c.intraRefreshCnt = count;
                c.h264VUIParameters = vui;
            }
            VideoCodec::Hevc => {
                native.profileGUID = NV_ENC_HEVC_PROFILE_MAIN_GUID;
                let c = &mut native.encodeCodecConfig.hevcConfig;
                c.idrPeriod = NVENC_INFINITE_GOPLENGTH;
                c.set_repeatSPSPPS(1);
                c.set_disableSPSPPS(0);
                c.set_chromaFormatIDC(1);
                c.inputBitDepth = NV_ENC_BIT_DEPTH_8;
                c.outputBitDepth = NV_ENC_BIT_DEPTH_8;
                c.set_enableIntraRefresh(enabled);
                c.intraRefreshPeriod = period;
                c.intraRefreshCnt = count;
                c.hevcVUIParameters = vui;
            }
            VideoCodec::Av1 => {
                native.profileGUID = NV_ENC_AV1_PROFILE_MAIN_GUID;
                let c = &mut native.encodeCodecConfig.av1Config;
                c.idrPeriod = NVENC_INFINITE_GOPLENGTH;
                c.set_repeatSeqHdr(1);
                c.set_outputAnnexBFormat(0);
                c.set_chromaFormatIDC(1);
                c.inputBitDepth = NV_ENC_BIT_DEPTH_8;
                c.outputBitDepth = NV_ENC_BIT_DEPTH_8;
                c.set_enableIntraRefresh(enabled);
                c.intraRefreshPeriod = period;
                c.intraRefreshCnt = count;
                c.colorPrimaries = NV_ENC_VUI_COLOR_PRIMARIES_BT709;
                c.transferCharacteristics = NV_ENC_VUI_TRANSFER_CHARACTERISTIC_BT709;
                c.matrixCoefficients = NV_ENC_VUI_MATRIX_COEFFS_BT709;
                c.colorRange = 0;
            }
        }
    }
    Ok(())
}
/// O chamador liga encodeConfig apenas durante Initialize/Reconfigure, sem ponteiro autorreferente persistente.
pub fn initialize_params(config: &EncoderConfig) -> NV_ENC_INITIALIZE_PARAMS {
    NV_ENC_INITIALIZE_PARAMS {
        version: NV_ENC_INITIALIZE_PARAMS_VER,
        encodeGUID: config.codec.guid(),
        presetGUID: config.preset.guid(),
        encodeWidth: config.width,
        encodeHeight: config.height,
        darWidth: config.width,
        darHeight: config.height,
        frameRateNum: config.fps.num,
        frameRateDen: config.fps.den,
        enableEncodeAsync: 0,
        enablePTD: 1,
        maxEncodeWidth: config.width,
        maxEncodeHeight: config.height,
        tuningInfo: NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY,
        ..Default::default()
    }
}
pub fn status_name(code: NVENCSTATUS) -> &'static str {
    match code {
        NV_ENC_SUCCESS => "NV_ENC_SUCCESS",
        NV_ENC_ERR_NO_ENCODE_DEVICE => "NV_ENC_ERR_NO_ENCODE_DEVICE",
        NV_ENC_ERR_UNSUPPORTED_DEVICE => "NV_ENC_ERR_UNSUPPORTED_DEVICE",
        NV_ENC_ERR_INVALID_ENCODERDEVICE => "NV_ENC_ERR_INVALID_ENCODERDEVICE",
        NV_ENC_ERR_INVALID_DEVICE => "NV_ENC_ERR_INVALID_DEVICE",
        NV_ENC_ERR_DEVICE_NOT_EXIST => "NV_ENC_ERR_DEVICE_NOT_EXIST",
        NV_ENC_ERR_INVALID_PTR => "NV_ENC_ERR_INVALID_PTR",
        NV_ENC_ERR_INVALID_EVENT => "NV_ENC_ERR_INVALID_EVENT",
        NV_ENC_ERR_INVALID_PARAM => "NV_ENC_ERR_INVALID_PARAM",
        NV_ENC_ERR_INVALID_CALL => "NV_ENC_ERR_INVALID_CALL",
        NV_ENC_ERR_OUT_OF_MEMORY => "NV_ENC_ERR_OUT_OF_MEMORY",
        NV_ENC_ERR_ENCODER_NOT_INITIALIZED => "NV_ENC_ERR_ENCODER_NOT_INITIALIZED",
        NV_ENC_ERR_UNSUPPORTED_PARAM => "NV_ENC_ERR_UNSUPPORTED_PARAM",
        NV_ENC_ERR_LOCK_BUSY => "NV_ENC_ERR_LOCK_BUSY",
        NV_ENC_ERR_NOT_ENOUGH_BUFFER => "NV_ENC_ERR_NOT_ENOUGH_BUFFER",
        NV_ENC_ERR_INVALID_VERSION => "NV_ENC_ERR_INVALID_VERSION",
        NV_ENC_ERR_MAP_FAILED => "NV_ENC_ERR_MAP_FAILED",
        NV_ENC_ERR_NEED_MORE_INPUT => "NV_ENC_ERR_NEED_MORE_INPUT",
        NV_ENC_ERR_ENCODER_BUSY => "NV_ENC_ERR_ENCODER_BUSY",
        NV_ENC_ERR_EVENT_NOT_REGISTERD => "NV_ENC_ERR_EVENT_NOT_REGISTERD",
        NV_ENC_ERR_GENERIC => "NV_ENC_ERR_GENERIC",
        NV_ENC_ERR_INCOMPATIBLE_CLIENT_KEY => "NV_ENC_ERR_INCOMPATIBLE_CLIENT_KEY",
        NV_ENC_ERR_UNIMPLEMENTED => "NV_ENC_ERR_UNIMPLEMENTED",
        NV_ENC_ERR_RESOURCE_REGISTER_FAILED => "NV_ENC_ERR_RESOURCE_REGISTER_FAILED",
        NV_ENC_ERR_RESOURCE_NOT_REGISTERED => "NV_ENC_ERR_RESOURCE_NOT_REGISTERED",
        NV_ENC_ERR_RESOURCE_NOT_MAPPED => "NV_ENC_ERR_RESOURCE_NOT_MAPPED",
        NV_ENC_ERR_NEED_MORE_OUTPUT => "NV_ENC_ERR_NEED_MORE_OUTPUT",
        _ => "NV_ENC_ERR_UNKNOWN",
    }
}
pub fn check(operation: &'static str, code: NVENCSTATUS) -> Result<()> {
    if code == NV_ENC_SUCCESS {
        return Ok(());
    }
    Err(crate::EncodeError::Nvenc {
        operation,
        status: status_name(code),
        code,
        hint: if matches!(
            code,
            NV_ENC_ERR_UNSUPPORTED_DEVICE
                | NV_ENC_ERR_INVALID_VERSION
                | NV_ENC_ERR_NO_ENCODE_DEVICE
        ) {
            "confira GPU NVIDIA, adapter da captura e driver compatível com NVENC API 12.2"
        } else {
            "consulte docs/encode.md e o código retornado pelo driver"
        },
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn laboratory_main_changes_real_encoder_profile_without_lookahead() -> Result<()> {
        let config = EncoderConfig {
            h264_profile: crate::config::H264Profile::Main,
            ..Default::default()
        };
        let mut native = NV_ENC_CONFIG::default();
        apply_config(&config, &mut native)?;
        let actual = native.profileGUID;
        let expected = NV_ENC_H264_PROFILE_MAIN_GUID;
        assert_eq!(
            (actual.Data1, actual.Data2, actual.Data3, actual.Data4),
            (
                expected.Data1,
                expected.Data2,
                expected.Data3,
                expected.Data4
            )
        );
        // SAFETY: apply_config initialized the H264 variant for this config.
        assert_eq!(
            unsafe { native.encodeCodecConfig.h264Config.adaptiveTransformMode },
            NV_ENC_H264_ADAPTIVE_TRANSFORM_DISABLE
        );
        assert_eq!(native.rcParams.enableLookahead(), 0);
        assert_eq!(native.frameIntervalP, 1);
        Ok(())
    }
    #[test]
    fn latency_policy_preserves_preset() -> Result<()> {
        for codec in [VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1] {
            let c = EncoderConfig {
                codec,
                ..Default::default()
            };
            let mut native = NV_ENC_CONFIG {
                mvPrecision: NV_ENC_MV_PRECISION_QUARTER_PEL,
                ..Default::default()
            };
            native.rcParams.set_enableLookahead(1);
            native.rcParams.set_enableAQ(1);
            apply_config(&c, &mut native)?;
            assert_eq!(native.mvPrecision, NV_ENC_MV_PRECISION_QUARTER_PEL);
            assert_eq!(native.frameIntervalP, 1);
            assert_eq!(native.gopLength, u32::MAX);
            assert_eq!(native.rcParams.vbvBufferSize, 125000);
            assert_eq!(native.rcParams.zeroReorderDelay(), 1);
            assert_eq!(native.rcParams.enableLookahead(), 0);
            assert_eq!(native.rcParams.enableAQ(), 0);
            assert_eq!(native.rcParams.enableTemporalAQ(), 0);
            assert_eq!(native.rcParams.multiPass, NV_ENC_MULTI_PASS_DISABLED);
            let init = initialize_params(&c);
            assert!(init.encodeConfig.is_null());
            assert_eq!(init.enableEncodeAsync, 0);
            assert_eq!(init.tuningInfo, NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY);
            // SAFETY: cada braço lê apenas a configuração do codec escrito por apply_config.
            unsafe {
                match codec {
                    VideoCodec::H264 => {
                        assert_eq!(native.encodeCodecConfig.h264Config.repeatSPSPPS(), 1)
                    }
                    VideoCodec::Hevc => {
                        assert_eq!(native.encodeCodecConfig.hevcConfig.repeatSPSPPS(), 1)
                    }
                    VideoCodec::Av1 => {
                        assert_eq!(native.encodeCodecConfig.av1Config.repeatSeqHdr(), 1);
                        assert_eq!(native.encodeCodecConfig.av1Config.outputAnnexBFormat(), 0);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Inicializa usando a tabela do driver, preservando a configuração retornada pelo preset.
///
/// # Safety
/// `handle` deve ser uma sessão NVENC viva de `functions`, sem inicialização prévia.
/// Os ponteiros da tabela devem ter as assinaturas do header 12.2 e permanecer válidos.
pub unsafe fn initialize_driver(
    functions: &NV_ENCODE_API_FUNCTION_LIST,
    handle: *mut core::ffi::c_void,
    config: &EncoderConfig,
) -> Result<NV_ENC_CONFIG> {
    // SAFETY: contrato idêntico, preserva inicialização síncrona dos consumidores existentes.
    unsafe { initialize_driver_mode(functions, handle, config, false) }
}
/// Inicialização com eventos assíncronos no Windows.
///
/// # Safety
/// Mesmo contrato de `initialize_driver`; modo async exige eventos registrados por slot.
pub unsafe fn initialize_driver_mode(
    functions: &NV_ENCODE_API_FUNCTION_LIST,
    handle: *mut core::ffi::c_void,
    config: &EncoderConfig,
    asynchronous: bool,
) -> Result<NV_ENC_CONFIG> {
    config.validate()?;
    let missing = || crate::EncodeError::EncoderUnavailable("tabela NVENC incompleta".into());
    let get_preset = functions.nvEncGetEncodePresetConfigEx.ok_or_else(missing)?;
    let initialize = functions.nvEncInitializeEncoder.ok_or_else(missing)?;
    let mut preset = NV_ENC_PRESET_CONFIG {
        version: NV_ENC_PRESET_CONFIG_VER,
        presetCfg: NV_ENC_CONFIG {
            version: NV_ENC_CONFIG_VER,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: contrato da função garante sessão/tabela; saída versionada, zerada e válida.
    check("NvEncGetEncodePresetConfigEx", unsafe {
        get_preset(
            handle,
            config.codec.guid(),
            config.preset.guid(),
            NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY,
            &mut preset,
        )
    })?;
    let mut native = preset.presetCfg;
    apply_config(config, &mut native)?;
    let mut init = initialize_params(config);
    init.enableEncodeAsync = u32::from(asynchronous);
    init.encodeConfig = &mut native;
    // SAFETY: ponteiro para native fica vivo até a chamada retornar; nenhuma autorreferência persistida.
    check("NvEncInitializeEncoder", unsafe {
        initialize(handle, &mut init)
    })?;
    Ok(native)
}
/// Reconfigura somente rate/fps/VBV, sem reset e sem IDR.
///
/// # Safety
/// `handle` e `functions` devem pertencer à sessão viva que gerou `native`, sem encode pendente.
pub unsafe fn reconfigure_driver(
    functions: &NV_ENCODE_API_FUNCTION_LIST,
    handle: *mut core::ffi::c_void,
    config: &EncoderConfig,
    native: NV_ENC_CONFIG,
) -> Result<NV_ENC_CONFIG> {
    // SAFETY: contrato preservado para consumidores síncronos.
    unsafe { reconfigure_driver_mode(functions, handle, config, native, false) }
}
/// Reconfigura preservando o modo de eventos da sessão.
///
/// # Safety
/// Mesmo contrato de `reconfigure_driver`; asynchronous deve corresponder à sessão.
pub unsafe fn reconfigure_driver_mode(
    functions: &NV_ENCODE_API_FUNCTION_LIST,
    handle: *mut core::ffi::c_void,
    config: &EncoderConfig,
    native: NV_ENC_CONFIG,
    asynchronous: bool,
) -> Result<NV_ENC_CONFIG> {
    let reconfigure = functions
        .nvEncReconfigureEncoder
        .ok_or_else(|| crate::EncodeError::EncoderUnavailable("reconfigure ausente".into()))?;
    let mut updated = native;
    apply_config(config, &mut updated)?;
    let mut params = NV_ENC_RECONFIGURE_PARAMS {
        version: NV_ENC_RECONFIGURE_PARAMS_VER,
        reInitEncodeParams: initialize_params(config),
        ..Default::default()
    };
    params.reInitEncodeParams.enableEncodeAsync = u32::from(asynchronous);
    params.reInitEncodeParams.encodeConfig = &mut updated;
    params.set_resetEncoder(0);
    params.set_forceIDR(0);
    // SAFETY: sessão ociosa, tabela validada pelo chamador e ponteiros locais válidos até retornar.
    check("NvEncReconfigureEncoder", unsafe {
        reconfigure(handle, &mut params)
    })?;
    Ok(updated)
}

#[cfg(test)]
mod driver_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    static STEP: AtomicU32 = AtomicU32::new(0);
    unsafe extern "C" fn preset(
        _: *mut core::ffi::c_void,
        codec: GUID,
        guid: GUID,
        tuning: NV_ENC_TUNING_INFO,
        out: *mut NV_ENC_PRESET_CONFIG,
    ) -> NVENCSTATUS {
        if out.is_null()
            || codec.Data1 != NV_ENC_CODEC_H264_GUID.Data1
            || guid.Data1 != NV_ENC_PRESET_P3_GUID.Data1
            || tuning != NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY
        {
            return NV_ENC_ERR_INVALID_PARAM;
        }
        // SAFETY: função de teste recebe saída válida de initialize_driver; ponteiro verificado acima.
        let out = unsafe { &mut *out };
        if out.version != 0xf205000c
            || out.presetCfg.version != 0xf209000c
            || STEP
                .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return NV_ENC_ERR_INVALID_CALL;
        }
        out.presetCfg.mvPrecision = NV_ENC_MV_PRECISION_QUARTER_PEL;
        out.presetCfg.encodeCodecConfig.h264Config = NV_ENC_CONFIG_H264 {
            level: 41,
            ..Default::default()
        };
        out.presetCfg.rcParams.set_enableAQ(1);
        out.presetCfg.rcParams.set_enableLookahead(1);
        NV_ENC_SUCCESS
    }
    unsafe extern "C" fn initialize(
        _: *mut core::ffi::c_void,
        params: *mut NV_ENC_INITIALIZE_PARAMS,
    ) -> NVENCSTATUS {
        if params.is_null() {
            return NV_ENC_ERR_INVALID_PTR;
        }
        // SAFETY: ponteiro local válido criado por initialize_driver.
        let init = unsafe { &*params };
        if init.encodeConfig.is_null() {
            return NV_ENC_ERR_INVALID_PTR;
        }
        // SAFETY: native vive na pilha do chamador por toda esta chamada síncrona.
        let c = unsafe { &*init.encodeConfig };
        // SAFETY: teste solicita preset H.264 e a union foi escrita pelo stub preset.
        let codec = unsafe { c.encodeCodecConfig.h264Config };
        if init.version != 0xf207000c
            || init.enableEncodeAsync != 0
            || init.enablePTD != 1
            || init.frameRateNum != 144
            || init.frameRateDen != 1
            || c.gopLength != u32::MAX
            || c.frameIntervalP != 1
            || c.mvPrecision != NV_ENC_MV_PRECISION_QUARTER_PEL
            || c.rcParams.rateControlMode != NV_ENC_PARAMS_RC_CBR
            || c.rcParams.enableAQ() != 0
            || c.rcParams.enableLookahead() != 0
            || c.rcParams.zeroReorderDelay() != 1
            || c.rcParams.vbvBufferSize != 125000
            // Sem h264_level no EncoderConfig o driver escolhe o nivel (o 41 do preset nao vale).
            || codec.level != NV_ENC_LEVEL_AUTOSELECT as u32
            || codec.repeatSPSPPS() != 1
            || c.reserved.iter().any(|&v| v != 0)
            || init.reserved1.iter().any(|&v| v != 0)
            || STEP
                .compare_exchange(1, 2, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return NV_ENC_ERR_INVALID_PARAM;
        }
        NV_ENC_SUCCESS
    }
    unsafe extern "C" fn reconfigure(
        _: *mut core::ffi::c_void,
        params: *mut NV_ENC_RECONFIGURE_PARAMS,
    ) -> NVENCSTATUS {
        if params.is_null() {
            return NV_ENC_ERR_INVALID_PTR;
        }
        // SAFETY: estrutura versionada com encodeConfig local, válida durante reconfigure_driver.
        let p = unsafe { &*params };
        if p.reInitEncodeParams.encodeConfig.is_null() {
            return NV_ENC_ERR_INVALID_PTR;
        }
        // SAFETY: native local criado por reconfigure_driver.
        let c = unsafe { &*p.reInitEncodeParams.encodeConfig };
        if p.version != 0xf202000c
            || p.resetEncoder() != 0
            || p.forceIDR() != 0
            || c.rcParams.averageBitRate != 12_000_000
            || c.rcParams.vbvBufferSize != 200200
            || p.reInitEncodeParams.frameRateNum != 60000
            || p.reInitEncodeParams.frameRateDen != 1001
            || STEP
                .compare_exchange(2, 3, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return NV_ENC_ERR_INVALID_PARAM;
        }
        NV_ENC_SUCCESS
    }
    #[test]
    fn fake_function_table_initializes_and_reconfigures_without_gpu() -> Result<()> {
        let table = NV_ENCODE_API_FUNCTION_LIST {
            nvEncGetEncodePresetConfigEx: Some(preset),
            nvEncInitializeEncoder: Some(initialize),
            nvEncReconfigureEncoder: Some(reconfigure),
            ..Default::default()
        };
        let mut config = EncoderConfig {
            preset: crate::Preset::P3,
            ..Default::default()
        };
        // SAFETY: stubs acima não usam handle; tabela tem assinaturas do header e saídas locais válidas.
        let native = unsafe { initialize_driver(&table, core::ptr::null_mut(), &config)? };
        assert_eq!(STEP.load(Ordering::SeqCst), 2);
        config.bitrate_bps = 12_000_000;
        config.fps = crate::FrameRate::new(60000, 1001)?;
        // SAFETY: continuação da sessão falsa acima, sem recursos GPU ou threads concorrentes.
        let updated =
            unsafe { reconfigure_driver(&table, core::ptr::null_mut(), &config, native)? };
        assert_eq!(STEP.load(Ordering::SeqCst), 3);
        assert_eq!(updated.rcParams.vbvBufferSize, 200200);
        Ok(())
    }
}

/// Tenta lock sem perder LOCK_BUSY; nenhuma espera é escondida no retry.
///
/// # Safety
/// Sessão, output e tabela devem estar vivos e pertencer ao mesmo encoder.
/// `nonblocking` exige sessão assíncrona e evento concluído no Windows.
pub unsafe fn lock_bitstream(
    functions: &NV_ENCODE_API_FUNCTION_LIST,
    handle: *mut core::ffi::c_void,
    lock: &mut NV_ENC_LOCK_BITSTREAM,
    nonblocking: bool,
) -> Result<bool> {
    lock.set_doNotWait(u32::from(nonblocking));
    let function = functions
        .nvEncLockBitstream
        .ok_or_else(|| crate::EncodeError::EncoderUnavailable("lock ausente".into()))?;
    // SAFETY: contrato do chamador garante sessão/output/tabela válidos.
    let status = unsafe { function(handle, lock) };
    if nonblocking && status == NV_ENC_ERR_LOCK_BUSY {
        return Ok(false);
    }
    check("NvEncLockBitstream", status)?;
    Ok(true)
}
#[cfg(test)]
mod lock_tests {
    use super::*;
    unsafe extern "C" fn busy(
        _: *mut core::ffi::c_void,
        params: *mut NV_ENC_LOCK_BITSTREAM,
    ) -> NVENCSTATUS {
        // SAFETY: ponteiro local válido fornecido por lock_bitstream no teste.
        if unsafe { (*params).doNotWait() } == 1 {
            NV_ENC_ERR_LOCK_BUSY
        } else {
            NV_ENC_SUCCESS
        }
    }
    #[test]
    fn fake_nonblocking_fifo_never_consumes_or_reorders_a_busy_slot() -> Result<()> {
        let functions = NV_ENCODE_API_FUNCTION_LIST {
            nvEncLockBitstream: Some(busy),
            ..Default::default()
        };
        let mut queue = crate::pipeline::PendingQueue::new(3);
        for slot in 0..3 {
            queue.push_back(slot)?;
        }
        assert!(matches!(queue.push_back(3), Err(crate::EncodeError::Busy)));
        let mut lock = NV_ENC_LOCK_BITSTREAM {
            version: NV_ENC_LOCK_BITSTREAM_VER,
            ..Default::default()
        };
        for _ in 0..3 {
            // SAFETY: fake usa somente params local, sem dereferenciar a sessão.
            if unsafe { lock_bitstream(&functions, core::ptr::null_mut(), &mut lock, true)? } {
                queue.pop_front();
            }
            assert_eq!(queue.front(), Some(&0));
            assert_eq!(queue.len(), 3);
        }
        for slot in 0..3 {
            // SAFETY: mesma tabela fake e armazenamento local, agora em drenagem bloqueante.
            assert!(unsafe {
                lock_bitstream(&functions, core::ptr::null_mut(), &mut lock, false)?
            });
            assert_eq!(queue.pop_front(), Some(slot));
        }
        assert!(queue.is_empty());
        Ok(())
    }
    #[test]
    fn fake_lock_busy_preserves_pending_and_blocking_drain_uses_zero() -> Result<()> {
        let functions = NV_ENCODE_API_FUNCTION_LIST {
            nvEncLockBitstream: Some(busy),
            ..Default::default()
        };
        let mut lock = NV_ENC_LOCK_BITSTREAM {
            version: NV_ENC_LOCK_BITSTREAM_VER,
            ..Default::default()
        };
        // SAFETY: fake não usa a sessão; params é local e vivo.
        unsafe {
            for _ in 0..3 {
                assert!(!lock_bitstream(
                    &functions,
                    core::ptr::null_mut(),
                    &mut lock,
                    true
                )?);
            }
            assert!(lock_bitstream(
                &functions,
                core::ptr::null_mut(),
                &mut lock,
                false
            )?);
        }
        Ok(())
    }
}

#[cfg(test)]
mod async_tests {
    use super::*;
    unsafe extern "C" fn preset(
        _: *mut core::ffi::c_void,
        _: GUID,
        _: GUID,
        _: NV_ENC_TUNING_INFO,
        output: *mut NV_ENC_PRESET_CONFIG,
    ) -> NVENCSTATUS {
        // SAFETY: initialize_driver_mode fornece descriptor local válido e versionado.
        unsafe {
            (*output).presetCfg.mvPrecision = NV_ENC_MV_PRECISION_QUARTER_PEL;
        }
        NV_ENC_SUCCESS
    }
    unsafe extern "C" fn initialize(
        _: *mut core::ffi::c_void,
        params: *mut NV_ENC_INITIALIZE_PARAMS,
    ) -> NVENCSTATUS {
        // SAFETY: descriptor e encodeConfig locais vivos durante initialize_driver_mode.
        let params = unsafe { &*params };
        if params.enableEncodeAsync != 1 || params.enablePTD != 1 {
            return NV_ENC_ERR_INVALID_PARAM;
        }
        // SAFETY: inicializador mantém config válida durante esta chamada.
        let config = unsafe { &*params.encodeConfig };
        if config.frameIntervalP != 1 || config.mvPrecision != NV_ENC_MV_PRECISION_QUARTER_PEL {
            return NV_ENC_ERR_INVALID_PARAM;
        }
        NV_ENC_SUCCESS
    }
    unsafe extern "C" fn always_busy(
        _: *mut core::ffi::c_void,
        _: *mut NV_ENC_LOCK_BITSTREAM,
    ) -> NVENCSTATUS {
        NV_ENC_ERR_LOCK_BUSY
    }
    unsafe extern "C" fn invalid(
        _: *mut core::ffi::c_void,
        _: *mut NV_ENC_LOCK_BITSTREAM,
    ) -> NVENCSTATUS {
        NV_ENC_ERR_INVALID_PARAM
    }
    #[test]
    fn async_initialization_preserves_preset_and_only_nonblocking_busy_is_retryable() -> Result<()>
    {
        let mut table = NV_ENCODE_API_FUNCTION_LIST {
            nvEncGetEncodePresetConfigEx: Some(preset),
            nvEncInitializeEncoder: Some(initialize),
            nvEncLockBitstream: Some(always_busy),
            ..Default::default()
        };
        let config = EncoderConfig::default();
        let mut lock = NV_ENC_LOCK_BITSTREAM {
            version: NV_ENC_LOCK_BITSTREAM_VER,
            ..Default::default()
        };
        // SAFETY: tabela fake ignora sessão, recebe somente descriptors locais válidos.
        unsafe {
            initialize_driver_mode(&table, core::ptr::null_mut(), &config, true)?;
            assert!(!lock_bitstream(
                &table,
                core::ptr::null_mut(),
                &mut lock,
                true
            )?);
            assert!(lock_bitstream(&table, core::ptr::null_mut(), &mut lock, false).is_err());
            table.nvEncLockBitstream = Some(invalid);
            assert!(lock_bitstream(&table, core::ptr::null_mut(), &mut lock, true).is_err());
        }
        Ok(())
    }
}
