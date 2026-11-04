//! Macros não emitidos pelo bindgen; valores de nv-codec-headers n12.2.72.0.
pub const fn struct_version(revision: u32) -> u32 {
    super::nvenc_sys::NVENCAPI_VERSION | (revision << 16) | (7 << 28)
}
pub const NV_ENC_CAPS_PARAM_VER: u32 = struct_version(1);
pub const NV_ENC_RESTORE_ENCODER_STATE_PARAMS_VER: u32 = struct_version(2);
pub const NV_ENC_OUTPUT_STATS_BLOCK_VER: u32 = struct_version(1);
pub const NV_ENC_OUTPUT_STATS_ROW_VER: u32 = struct_version(1);
pub const NV_ENC_ENCODE_OUT_PARAMS_VER: u32 = struct_version(1);
pub const NV_ENC_LOOKAHEAD_PIC_PARAMS_VER: u32 = struct_version(2);
pub const NV_ENC_CREATE_INPUT_BUFFER_VER: u32 = struct_version(2);
pub const NV_ENC_CREATE_BITSTREAM_BUFFER_VER: u32 = struct_version(1);
pub const NV_ENC_CREATE_MV_BUFFER_VER: u32 = struct_version(2);
pub const NV_ENC_RC_PARAMS_VER: u32 = struct_version(1);
pub const NV_ENC_CONFIG_VER: u32 = struct_version(9) | (1 << 31);
pub const NV_ENC_INITIALIZE_PARAMS_VER: u32 = struct_version(7) | (1 << 31);
pub const NV_ENC_RECONFIGURE_PARAMS_VER: u32 = struct_version(2) | (1 << 31);
pub const NV_ENC_PRESET_CONFIG_VER: u32 = struct_version(5) | (1 << 31);
pub const NV_ENC_PIC_PARAMS_MVC_VER: u32 = struct_version(1);
pub const NV_ENC_PIC_PARAMS_VER: u32 = struct_version(7) | (1 << 31);
pub const NV_ENC_MEONLY_PARAMS_VER: u32 = struct_version(4);
pub const NV_ENC_LOCK_BITSTREAM_VER: u32 = struct_version(2) | (1 << 31);
pub const NV_ENC_LOCK_INPUT_BUFFER_VER: u32 = struct_version(1);
pub const NV_ENC_MAP_INPUT_RESOURCE_VER: u32 = struct_version(4);
pub const NV_ENC_FENCE_POINT_D3D12_VER: u32 = struct_version(1);
pub const NV_ENC_INPUT_RESOURCE_D3D12_VER: u32 = struct_version(1);
pub const NV_ENC_OUTPUT_RESOURCE_D3D12_VER: u32 = struct_version(1);
pub const NV_ENC_REGISTER_RESOURCE_VER: u32 = struct_version(5);
pub const NV_ENC_STAT_VER: u32 = struct_version(2);
pub const NV_ENC_SEQUENCE_PARAM_PAYLOAD_VER: u32 = struct_version(1);
pub const NV_ENC_EVENT_PARAMS_VER: u32 = struct_version(2);
pub const NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER: u32 = struct_version(1);
pub const NV_ENCODE_API_FUNCTION_LIST_VER: u32 = struct_version(2);
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_match_header() {
        assert_eq!(NV_ENC_CAPS_PARAM_VER, 0x7201000c);
        assert_eq!(NV_ENC_RESTORE_ENCODER_STATE_PARAMS_VER, 0x7202000c);
        assert_eq!(NV_ENC_OUTPUT_STATS_BLOCK_VER, 0x7201000c);
        assert_eq!(NV_ENC_OUTPUT_STATS_ROW_VER, 0x7201000c);
        assert_eq!(NV_ENC_ENCODE_OUT_PARAMS_VER, 0x7201000c);
        assert_eq!(NV_ENC_LOOKAHEAD_PIC_PARAMS_VER, 0x7202000c);
        assert_eq!(NV_ENC_CREATE_INPUT_BUFFER_VER, 0x7202000c);
        assert_eq!(NV_ENC_CREATE_BITSTREAM_BUFFER_VER, 0x7201000c);
        assert_eq!(NV_ENC_CREATE_MV_BUFFER_VER, 0x7202000c);
        assert_eq!(NV_ENC_RC_PARAMS_VER, 0x7201000c);
        assert_eq!(NV_ENC_CONFIG_VER, 0xf209000c);
        assert_eq!(NV_ENC_INITIALIZE_PARAMS_VER, 0xf207000c);
        assert_eq!(NV_ENC_RECONFIGURE_PARAMS_VER, 0xf202000c);
        assert_eq!(NV_ENC_PRESET_CONFIG_VER, 0xf205000c);
        assert_eq!(NV_ENC_PIC_PARAMS_MVC_VER, 0x7201000c);
        assert_eq!(NV_ENC_PIC_PARAMS_VER, 0xf207000c);
        assert_eq!(NV_ENC_MEONLY_PARAMS_VER, 0x7204000c);
        assert_eq!(NV_ENC_LOCK_BITSTREAM_VER, 0xf202000c);
        assert_eq!(NV_ENC_LOCK_INPUT_BUFFER_VER, 0x7201000c);
        assert_eq!(NV_ENC_MAP_INPUT_RESOURCE_VER, 0x7204000c);
        assert_eq!(NV_ENC_FENCE_POINT_D3D12_VER, 0x7201000c);
        assert_eq!(NV_ENC_INPUT_RESOURCE_D3D12_VER, 0x7201000c);
        assert_eq!(NV_ENC_OUTPUT_RESOURCE_D3D12_VER, 0x7201000c);
        assert_eq!(NV_ENC_REGISTER_RESOURCE_VER, 0x7205000c);
        assert_eq!(NV_ENC_STAT_VER, 0x7202000c);
        assert_eq!(NV_ENC_SEQUENCE_PARAM_PAYLOAD_VER, 0x7201000c);
        assert_eq!(NV_ENC_EVENT_PARAMS_VER, 0x7202000c);
        assert_eq!(NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER, 0x7201000c);
        assert_eq!(NV_ENCODE_API_FUNCTION_LIST_VER, 0x7202000c);
        assert_eq!(super::super::nvenc_sys::NVENCAPI_VERSION, 0x0200000c);
        assert_eq!(
            super::super::nvenc_sys::NVENCAPI_MAJOR_VERSION << 4
                | super::super::nvenc_sys::NVENCAPI_MINOR_VERSION,
            0xc2
        );
    }
}

/// O formato retornado por GetMaxSupportedVersion difere do campo de versão das structs.
pub fn check_driver_version(max_version: u32) -> crate::Result<()> {
    let required =
        super::nvenc_sys::NVENCAPI_MAJOR_VERSION << 4 | super::nvenc_sys::NVENCAPI_MINOR_VERSION;
    if max_version < required {
        return Err(crate::EncodeError::EncoderUnavailable(format!(
            "driver oferece API {}.{}, mas o executável exige 12.2; atualize o driver",
            max_version >> 4,
            max_version & 15
        )));
    }
    Ok(())
}
#[cfg(test)]
mod compatibility_tests {
    #[test]
    fn refuses_old_driver_before_create_instance() {
        assert!(super::check_driver_version(0xc1).is_err());
        assert!(super::check_driver_version(0xc2).is_ok());
        assert!(super::check_driver_version(0xd0).is_ok());
    }
}
