//! Análise offline de benchmarks; não faz captura nem depende de APIs do sistema.
pub mod model;
pub mod presentmon;
pub mod report;
pub mod stats;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("erro de arquivo: {0}")]
    Io(#[from] std::io::Error),
    #[error("CSV inválido: {0}")]
    Csv(#[from] csv::Error),
    #[error("JSON inválido: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}
