use crate::{EncodeError, FrameKind, Result, VideoCodec};
use std::io::{self, Write};

/// NALs sem start code; aceita misturar prefixos de 3/4 bytes, ignora padding final.
pub struct AnnexB<'a> {
    data: &'a [u8],
    offset: usize,
}
impl<'a> AnnexB<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }
}
fn start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
    (from..data.len().saturating_sub(2)).find_map(|i| {
        if data[i..].starts_with(&[0, 0, 0, 1]) {
            Some((i, 4))
        } else if data[i..].starts_with(&[0, 0, 1]) {
            Some((i, 3))
        } else {
            None
        }
    })
}
impl<'a> Iterator for AnnexB<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (start, prefix) = start_code(self.data, self.offset)?;
            let body = start + prefix;
            let mut end = start_code(self.data, body).map_or(self.data.len(), |(i, _)| i);
            self.offset = end;
            while end > body && self.data[end - 1] == 0 {
                end -= 1;
            }
            if body < end {
                return Some(&self.data[body..end]);
            }
            if self.offset >= self.data.len() {
                return None;
            }
        }
    }
}
#[derive(Debug, Default, PartialEq, Eq)]
pub struct NalInfo {
    pub key: bool,
    pub sps: bool,
    pub pps: bool,
    pub vps: bool,
}
pub fn inspect_annex_b(codec: VideoCodec, data: &[u8]) -> NalInfo {
    let mut info = NalInfo::default();
    for nal in AnnexB::new(data) {
        match codec {
            VideoCodec::H264 => match nal[0] & 31 {
                5 => info.key = true,
                7 => info.sps = true,
                8 => info.pps = true,
                _ => {}
            },
            VideoCodec::Hevc if nal.len() >= 2 => match (nal[0] >> 1) & 63 {
                16..=21 => info.key = true,
                32 => info.vps = true,
                33 => info.sps = true,
                34 => info.pps = true,
                _ => {}
            },
            _ => {}
        }
    }
    info
}
/// Remove o byte de prevenção de emulação (`00 00 03`) do RBSP.
fn unescape_rbsp(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut zeros = 0u8;
    for &byte in data {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        out.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
    }
    out
}

/// `profile-level-id` de um NAL SPS H.264 (sem start code): perfil, constraints e nível.
pub fn h264_sps_profile_level_id(nal: &[u8]) -> Option<String> {
    if nal.first().is_none_or(|header| header & 0x1f != 7) {
        return None;
    }
    let rbsp = unescape_rbsp(&nal[1..]);
    let (profile, constraints, level) = (*rbsp.first()?, *rbsp.get(1)?, *rbsp.get(2)?);
    Some(format!("{profile:02x}{constraints:02x}{level:02x}"))
}

/// Primeiro SPS de um access unit Annex B.
pub fn h264_access_unit_profile_level_id(data: &[u8]) -> Option<String> {
    AnnexB::new(data).find_map(h264_sps_profile_level_id)
}

pub fn validate_key_headers(codec: VideoCodec, data: &[u8]) -> Result<()> {
    if codec == VideoCodec::Av1 {
        let mut sequence = false;
        for obu in Obus::new(data) {
            sequence |= obu?.kind == 1;
        }
        return if sequence {
            Ok(())
        } else {
            Err(EncodeError::InvalidBitstream(
                "keyframe AV1 sem sequence header",
            ))
        };
    }
    let i = inspect_annex_b(codec, data);
    if !i.key || !i.sps || !i.pps || (codec == VideoCodec::Hevc && !i.vps) {
        return Err(EncodeError::InvalidBitstream(
            "keyframe sem SPS/PPS/VPS ou sem NAL intra",
        ));
    }
    Ok(())
}
#[derive(Debug)]
pub struct Obu<'a> {
    pub kind: u8,
    pub payload: &'a [u8],
}
/// AV1 low-overhead OBU stream (não Annex B), exatamente a saída selecionada no NVENC.
pub struct Obus<'a> {
    rest: &'a [u8],
}
impl<'a> Obus<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { rest: data }
    }
}
impl<'a> Iterator for Obus<'a> {
    type Item = Result<Obu<'a>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let result = parse_obu(self.rest);
        match result {
            Ok((obu, count)) => {
                self.rest = &self.rest[count..];
                Some(Ok(obu))
            }
            Err(e) => {
                self.rest = &[];
                Some(Err(e))
            }
        }
    }
}
fn parse_obu(data: &[u8]) -> Result<(Obu<'_>, usize)> {
    let invalid = || EncodeError::InvalidBitstream("OBU truncado ou inválido");
    let header = *data.first().ok_or_else(invalid)?;
    if header & 0x81 != 0 || header & 2 == 0 {
        return Err(invalid());
    }
    let mut pos = if header & 4 != 0 { 2 } else { 1 };
    if pos == 2 && data.get(1).is_none_or(|v| v & 7 != 0) {
        return Err(invalid());
    }
    let mut size = 0u64;
    let mut finished = false;
    for shift in (0..56).step_by(7) {
        let b = *data.get(pos).ok_or_else(invalid)?;
        pos += 1;
        size |= u64::from(b & 127) << shift;
        if b & 128 == 0 {
            finished = true;
            break;
        }
    }
    if !finished {
        return Err(invalid());
    }
    let end = pos
        .checked_add(usize::try_from(size).map_err(|_| invalid())?)
        .ok_or_else(invalid)?;
    let payload = data.get(pos..end).ok_or_else(invalid)?;
    Ok((
        Obu {
            kind: (header >> 3) & 15,
            payload,
        },
        end,
    ))
}
/// Estado mínimo do sequence header para identificar KEY_FRAME e show_existing_frame.
/// Flags do NVENC continuam sendo a fonte de verdade para a AU produzida.
#[derive(Default)]
pub struct Av1Inspector {
    reduced_still_picture: bool,
}
impl Av1Inspector {
    pub fn inspect(&mut self, data: &[u8]) -> Result<Option<FrameKind>> {
        for obu in Obus::new(data) {
            let obu = obu?;
            if obu.kind == 1 {
                let header = *obu
                    .payload
                    .first()
                    .ok_or(EncodeError::InvalidBitstream("sequence header vazio"))?;
                self.reduced_still_picture = header & 8 != 0;
            } else if matches!(obu.kind, 3 | 6) {
                if self.reduced_still_picture {
                    return Ok(Some(FrameKind::Key));
                }
                let b = *obu
                    .payload
                    .first()
                    .ok_or(EncodeError::InvalidBitstream("frame header vazio"))?;
                if b & 0x80 != 0 {
                    return Ok(Some(FrameKind::Delta));
                }
                return Ok(Some(if (b >> 5) & 3 == 0 {
                    FrameKind::Key
                } else {
                    FrameKind::Delta
                }));
            }
        }
        Ok(None)
    }
}
/// Para inspeção sem estado, a AU deve conter sequence header ou usar o perfil normal NVENC.
pub fn av1_frame_kind(data: &[u8]) -> Result<Option<FrameKind>> {
    Av1Inspector::default().inspect(data)
}

#[derive(Debug)]
pub struct IvfFrame<'a> {
    pub timestamp: u64,
    pub data: &'a [u8],
}
/// Leitor AV01/IVF sem alocação. A contagem no header pode ser zero para arquivo contínuo.
pub struct IvfFrames<'a> {
    rest: &'a [u8],
}
impl<'a> IvfFrames<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        if data.len() < 32
            || &data[..4] != b"DKIF"
            || data[4..8] != [0, 0, 32, 0]
            || &data[8..12] != b"AV01"
            || data[12..14] == [0, 0]
            || data[14..16] == [0, 0]
            || data[16..20] == [0, 0, 0, 0]
            || data[20..24] == [0, 0, 0, 0]
        {
            return Err(EncodeError::InvalidBitstream("header IVF AV01 inválido"));
        }
        Ok(Self { rest: &data[32..] })
    }
}
impl<'a> Iterator for IvfFrames<'a> {
    type Item = Result<IvfFrame<'a>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let parse = || -> Result<(IvfFrame<'a>, usize)> {
            let invalid = || EncodeError::InvalidBitstream("frame IVF truncado");
            let header = self.rest.get(..12).ok_or_else(invalid)?;
            let size = u32::from_le_bytes(header[..4].try_into().map_err(|_| invalid())?) as usize;
            let timestamp = u64::from_le_bytes(header[4..12].try_into().map_err(|_| invalid())?);
            let end = 12usize.checked_add(size).ok_or_else(invalid)?;
            let data = self.rest.get(12..end).ok_or_else(invalid)?;
            Ok((IvfFrame { timestamp, data }, end))
        };
        match parse() {
            Ok((frame, count)) => {
                self.rest = &self.rest[count..];
                Some(Ok(frame))
            }
            Err(e) => {
                self.rest = &[];
                Some(Err(e))
            }
        }
    }
}
pub struct AnnexBWriter<W: Write>(pub W);
impl<W: Write> AnnexBWriter<W> {
    pub fn write_au(&mut self, data: &[u8]) -> io::Result<()> {
        if start_code(data, 0).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "AU sem start code Annex B",
            ));
        }
        self.0.write_all(data)
    }
}
pub struct IvfWriter<W: Write> {
    writer: W,
    frames: u64,
}
impl<W: Write> IvfWriter<W> {
    /// Contagem zero no cabeçalho permite streaming sem Seek; frames têm timestamps racionais.
    pub fn new(mut writer: W, width: u32, height: u32, fps: crate::FrameRate) -> io::Result<Self> {
        if width == 0
            || height == 0
            || width > 65535
            || height > 65535
            || fps.num == 0
            || fps.den == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "dimensões/FPS IVF inválidos",
            ));
        }
        let mut header = [0u8; 32];
        header[..4].copy_from_slice(b"DKIF");
        header[6..8].copy_from_slice(&32u16.to_le_bytes());
        header[8..12].copy_from_slice(b"AV01");
        header[12..14].copy_from_slice(&(width as u16).to_le_bytes());
        header[14..16].copy_from_slice(&(height as u16).to_le_bytes());
        header[16..20].copy_from_slice(&fps.num.to_le_bytes());
        header[20..24].copy_from_slice(&fps.den.to_le_bytes());
        writer.write_all(&header)?;
        Ok(Self { writer, frames: 0 })
    }
    pub fn write_au(&mut self, data: &[u8], timestamp: u64) -> io::Result<()> {
        let size = u32::try_from(data.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "AU IVF >4 GiB"))?;
        self.writer.write_all(&size.to_le_bytes())?;
        self.writer.write_all(&timestamp.to_le_bytes())?;
        self.writer.write_all(data)?;
        self.frames += 1;
        Ok(())
    }
    pub fn frames(&self) -> u64 {
        self.frames
    }
    pub fn into_inner(self) -> W {
        self.writer
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_start_codes_and_headers() -> Result<()> {
        let data = [
            0, 0, 0, 1, 0x67, 1, 0, 0, 1, 0x68, 2, 0, 0, 0, 1, 0x65, 3, 0, 0,
        ];
        assert_eq!(
            AnnexB::new(&data).collect::<Vec<_>>(),
            vec![&[0x67, 1][..], &[0x68, 2], &[0x65, 3]]
        );
        let sps = [0, 0, 0, 1, 0x67, 0x64, 0x00, 0x28, 0, 0, 1, 0x68, 1];
        assert_eq!(
            h264_access_unit_profile_level_id(&sps).as_deref(),
            Some("640028")
        );
        // 00 00 03 é prevenção de emulação: o nível fica no byte seguinte do RBSP.
        assert_eq!(
            h264_sps_profile_level_id(&[0x67, 0x00, 0x00, 0x03, 0x2a]).as_deref(),
            Some("00002a")
        );
        assert_eq!(h264_sps_profile_level_id(&[0x68, 0x64, 0x00, 0x28]), None);
        assert_eq!(h264_access_unit_profile_level_id(&[0, 0, 1, 0x65, 1]), None);
        validate_key_headers(VideoCodec::H264, &data)?;
        assert!(validate_key_headers(VideoCodec::H264, &[0, 0, 1, 0x65]).is_err());
        assert_eq!(AnnexB::new(&[0, 0, 1, 0, 0, 1]).count(), 0);
        let hevc = [
            0, 0, 1, 64, 1, 0, 0, 1, 66, 1, 0, 0, 1, 68, 1, 0, 0, 1, 38, 1,
        ];
        validate_key_headers(VideoCodec::Hevc, &hevc)?;
        assert!(!inspect_annex_b(VideoCodec::Hevc, &[0, 0, 1, 2, 1]).key);
        Ok(())
    }
    #[test]
    fn obu_and_ivf() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let data = [0x0a, 1, 0, 0x32, 1, 0x10];
        validate_key_headers(VideoCodec::Av1, &data)?;
        assert_eq!(av1_frame_kind(&data)?, Some(FrameKind::Key));
        for invalid in [&[0x32][..], &[0x32, 127, 1], &[0xb2, 0], &[0x30, 0]] {
            assert!(Obus::new(invalid).next().is_some_and(|v| v.is_err()));
        }
        let mut w = IvfWriter::new(
            Vec::new(),
            1920,
            1080,
            crate::FrameRate {
                num: 60000,
                den: 1001,
            },
        )?;
        w.write_au(&data, 7)?;
        assert_eq!(w.frames(), 1);
        let b = w.into_inner();
        assert_eq!(&b[..4], b"DKIF");
        assert_eq!(&b[8..12], b"AV01");
        assert_eq!(&b[32..36], &6u32.to_le_bytes());
        assert_eq!(&b[36..44], &7u64.to_le_bytes());
        assert_eq!(&b[44..], &data);
        let mut reader = IvfFrames::new(&b)?;
        let frame = reader.next().ok_or("frame ausente")??;
        assert_eq!(frame.timestamp, 7);
        assert_eq!(av1_frame_kind(frame.data)?, Some(FrameKind::Key));
        assert!(reader.next().is_none());
        let mut truncated = IvfFrames::new(&b[..b.len() - 1])?;
        assert!(truncated.next().is_some_and(|r| r.is_err()));
        let mut inspector = Av1Inspector::default();
        assert_eq!(inspector.inspect(&[0x0a, 1, 0x18])?, None);
        assert_eq!(inspector.inspect(&[0x32, 1, 0x80])?, Some(FrameKind::Key));
        assert_eq!(av1_frame_kind(&[0x32, 1, 0x80])?, Some(FrameKind::Delta));
        assert_eq!(av1_frame_kind(&[0x32, 1, 0x20])?, Some(FrameKind::Delta));
        let mut annex = AnnexBWriter(Vec::new());
        assert!(annex.write_au(&data).is_err());
        Ok(())
    }
}
