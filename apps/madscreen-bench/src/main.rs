use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use madscreen_telemetry::{model, presentmon, report};
use std::{fs::File, path::PathBuf};

#[derive(Parser)]
#[command(version, about = "Análise reproduzível de overhead com PresentMon")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analisa manifest.json e os CSVs de um diretório.
    Analyze {
        dir: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 1.5)]
        gate_cv: f64,
        /// Retorna erro quando o gate de estabilidade do baseline falhar.
        #[arg(long)]
        strict: bool,
    },
    /// Resume um CSV; --app seleciona o executável.
    SummarizeCsv {
        file: PathBuf,
        #[arg(long)]
        app: Option<String>,
    },
    /// Imprime um exemplo de manifest (schema versão 1).
    Schema,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Schema => println!(
            "{}",
            serde_json::to_string_pretty(&model::example_manifest())?
        ),
        Command::SummarizeCsv { file, app } => {
            let capture = presentmon::parse(
                File::open(&file).context("não foi possível abrir o CSV")?,
                app.as_deref(),
            )?;
            let stats = capture.stats().context("CSV sem frames válidos")?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "stats": stats, "invalid_rows": capture.invalid_rows,
                    "invalid_optional_values": capture.invalid_optional_values,
                    "initial_rows": capture.initial_rows, "filtered_rows": capture.filtered_rows,
                    "streams": capture.streams, "applications": capture.applications,
                "application_filter_unavailable": capture.application_filter_unavailable,
                }))?
            );
        }
        Command::Analyze {
            dir,
            out,
            gate_cv,
            strict,
        } => {
            let manifest = serde_json::from_reader(
                File::open(dir.join("manifest.json"))
                    .context("não foi possível abrir manifest.json")?,
            )?;
            let result = report::analyze(&dir, manifest, gate_cv)?;
            let out = out.unwrap_or_else(|| dir.clone());
            report::write(&result, &out)?;
            println!(
                "Relatório gravado em {}. Gate do baseline: {}.",
                out.display(),
                if result.gate_pass { "PASS" } else { "FAIL" }
            );
            if strict && !result.gate_pass {
                bail!("gate de estabilidade do baseline falhou; consulte report.md");
            }
        }
    }
    Ok(())
}
