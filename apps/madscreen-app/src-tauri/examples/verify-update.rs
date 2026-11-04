//! Valida o pacote contra a chave embarcada antes de publicar; nunca lê a chave privada.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};
use serde_json::Value;
use std::{error::Error, fs, io, path::Path};

fn field<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("Campo de atualização ausente").into())
}

fn verify(manifest: &Path, artifact: &Path, config: &Path) -> Result<(), Box<dyn Error>> {
    let manifest: Value = serde_json::from_slice(&fs::read(manifest)?)?;
    let config: Value = serde_json::from_slice(&fs::read(config)?)?;
    if config
        .pointer("/plugins/updater/requireSignedVersion")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err(io::Error::other("Versão assinada deve ser obrigatória").into());
    }
    let public = String::from_utf8(STANDARD.decode(field(&config, "/plugins/updater/pubkey")?)?)?;
    let signature = String::from_utf8(
        STANDARD.decode(field(&manifest, "/platforms/windows-x86_64/signature")?)?,
    )?;
    let public = PublicKey::decode(&public)?;
    let signature = Signature::decode(&signature)?;
    public.verify(&fs::read(artifact)?, &signature, true)?;
    let announced = semver::Version::parse(field(&manifest, "/version")?.trim_start_matches('v'))?;
    let signed = signature
        .trusted_comment()
        .split('\t')
        .find_map(|part| part.strip_prefix("version:"))
        .ok_or_else(|| io::Error::other("Versão ausente na assinatura"))?;
    if semver::Version::parse(signed.trim_start_matches('v'))? != announced {
        return Err(io::Error::other("Versão anunciada difere da versão assinada").into());
    }
    println!("Assinatura e versão {announced} verificadas com a chave pública embarcada.");
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(
            io::Error::other("Uso: verify-update MANIFESTO INSTALADOR CONFIG_TAURI").into(),
        );
    }
    verify(
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
    )
}
