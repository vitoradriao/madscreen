//! Carregamento explícito de DLL, sem link estático ao driver.
#![allow(unsafe_code)]
use crate::{EncodeError, Result};
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{
            GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
            LoadLibraryExW, LoadLibraryW,
        },
    },
    core::{PCSTR, PCWSTR},
};
pub(crate) struct Library(HMODULE);
impl Library {
    pub fn load_path(path: &std::path::Path) -> Result<Self> {
        use std::os::windows::ffi::OsStrExt;
        if !path.is_absolute() {
            return Err(EncodeError::InvalidConfig(
                "caminho da DLL deve ser absoluto",
            ));
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: caminho absoluto NUL vivo; dependências resolvidas na pasta da DLL ou no sistema.
        unsafe {
            LoadLibraryExW(
                PCWSTR(wide.as_ptr()),
                None,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        }
        .map(Self)
        .map_err(|e| EncodeError::EncoderUnavailable(format!("{}: {e}", path.display())))
    }
    pub fn load(name: PCWSTR) -> Result<Self> {
        // SAFETY: nome estático NUL; handle possuído até Drop.
        unsafe { LoadLibraryW(name) }
            .map(Self)
            .map_err(|e| EncodeError::EncoderUnavailable(e.to_string()))
    }
    /// T deve ser a assinatura exata da função exportada.
    pub unsafe fn symbol<T: Copy>(&self, name: PCSTR) -> Result<T> {
        // SAFETY: DLL viva e nome NUL.
        let address = unsafe { GetProcAddress(self.0, name) }
            .ok_or_else(|| EncodeError::EncoderUnavailable("symbole du driver absent".into()))?;
        if std::mem::size_of::<T>() != std::mem::size_of_val(&address) {
            return Err(EncodeError::EncoderUnavailable(
                "taille de pointeur ABI invalide".into(),
            ));
        }
        // SAFETY: contrato exige T com assinatura exata do símbolo; tamanhos iguais.
        Ok(unsafe { std::mem::transmute_copy(&address) })
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: handle possuído, liberado uma vez após o uso das funções.
        let _ = unsafe { FreeLibrary(self.0) };
    }
}
