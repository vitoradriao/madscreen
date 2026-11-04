use crate::{
    Error,
    model::*,
    presentmon,
    stats::{Estimate, cv_percent, mean},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    path::{Component, Path},
};

fn status_label(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Complete => "concluída",
        RunStatus::Skipped => "pulada",
        RunStatus::Failed => "falhou",
    }
}

fn relative_file(root: &Path, name: &str) -> Result<std::path::PathBuf, Error> {
    let path = Path::new(name);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(Error::Invalid(
            "CSV deve ter caminho relativo sem '..'".into(),
        ));
    }
    Ok(root.join(path))
}

pub fn analyze(root: &Path, manifest: Manifest, gate_limit: f64) -> Result<Report, Error> {
    if !gate_limit.is_finite() || gate_limit < 0.0 {
        return Err(Error::Invalid(
            "--gate-cv deve ser finito e não negativo".into(),
        ));
    }
    if manifest.schema_version != 1
        || manifest.runs.is_empty()
        || manifest.runs_per_condition == 0
        || manifest.baseline_condition.is_empty()
        || !manifest.duration_sec.is_finite()
        || manifest.duration_sec <= 0.0
        || !manifest.warmup_sec.is_finite()
        || manifest.warmup_sec < 0.0
    {
        return Err(Error::Invalid(
            "manifest inválido: confira versão, corridas e durações".into(),
        ));
    }
    let mut identities = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut orders = BTreeSet::new();
    for entry in &manifest.runs {
        if entry.game.is_empty()
            || entry.scenario.is_empty()
            || entry.condition.is_empty()
            || entry.application.is_empty()
            || entry.round == 0
            || entry.order == 0
            || !entry.duration_sec.is_finite()
            || entry.duration_sec <= 0.0
            || !entry.warmup_sec.is_finite()
            || entry.warmup_sec < 0.0
            || !ids.insert(&entry.id)
            || !orders.insert(entry.order)
            || !identities.insert((&entry.game, &entry.scenario, &entry.condition, entry.round))
        {
            return Err(Error::Invalid(
                "manifest com corrida inválida ou identidade/ordem duplicada".into(),
            ));
        }
    }
    let mut warnings = Vec::new();
    if manifest.synthetic {
        warnings.push("Dados SINTÉTICOS: não comprovam o aceite do M0.3.".into());
    }
    if manifest.runs_per_condition != 5
        || manifest.duration_sec != 180.0
        || manifest.warmup_sec != 60.0
    {
        warnings.push("Protocolo diferente de 5 corridas × 180 s, aquecimento de 60 s.".into());
    }
    let mut runs = Vec::new();
    for entry in &manifest.runs {
        let mut run = RunAnalysis {
            entry: entry.clone(),
            stats: None,
            loss_percent: None,
            invalid_rows: 0,
            invalid_optional_values: 0,
            warnings: Vec::new(),
        };
        if entry.status != RunStatus::Complete {
            run.warnings.push(format!(
                "Corrida {}: {}",
                status_label(entry.status),
                entry.notes.as_deref().unwrap_or("sem observação")
            ));
        } else {
            let parsed = (|| {
                let csv = entry
                    .csv
                    .as_deref()
                    .ok_or_else(|| Error::Invalid("corrida completa sem CSV".into()))?;
                presentmon::parse(
                    File::open(relative_file(root, csv)?)?,
                    Some(&entry.application),
                )
            })();
            match parsed {
                Err(error) => run.warnings.push(format!("Falha ao analisar CSV: {error}")),
                Ok(capture) => {
                    if capture.application_filter_unavailable {
                        run.warnings.push("Application ausente: não foi possível verificar o executável; todos os frames foram considerados.".into());
                    }
                    run.invalid_rows = capture.invalid_rows;
                    run.invalid_optional_values = capture.invalid_optional_values;
                    if capture.invalid_rows > 0 {
                        run.warnings.push(format!(
                            "{} linhas inválidas descartadas",
                            capture.invalid_rows
                        ));
                    }
                    if capture.invalid_optional_values > 0 {
                        run.warnings.push(format!(
                            "{} métricas opcionais inválidas",
                            capture.invalid_optional_values
                        ));
                    }
                    if capture.streams > 1 {
                        run.warnings.push("Múltiplos processos/swapchains: FPS agregado; isole a swapchain principal antes do aceite.".into());
                    }
                    run.stats = capture.stats();
                    if let Some(stats) = &run.stats {
                        if stats.frames < 1000 {
                            run.warnings.push(
                                "Frames insuficientes (< 1000) para representar a cauda de 0,1%."
                                    .into(),
                            );
                        }
                        let tolerance = (manifest.duration_sec * 0.01).max(0.1);
                        if (stats.duration_sec - manifest.duration_sec).abs() > tolerance {
                            run.warnings.push(format!(
                                "Duração medida {:.3} s != esperada {:.3} s (tolerância {:.3} s).",
                                stats.duration_sec, manifest.duration_sec, tolerance
                            ));
                        }
                        if stats.mean_gpu_busy_ms.is_none() {
                            run.warnings.push("GPUBusy indisponível.".into());
                        }
                    } else {
                        run.warnings
                            .push("Nenhum frame válido para estatística.".into());
                    }
                }
            }
        }
        if entry.duration_sec != manifest.duration_sec || entry.warmup_sec != manifest.warmup_sec {
            run.warnings
                .push("Duração/aquecimento declarados diferem do manifest.".into());
        }
        runs.push(run);
    }
    let scenarios = manifest
        .runs
        .iter()
        .map(|r| (r.game.clone(), r.scenario.clone()))
        .collect::<BTreeSet<_>>();
    let mut gates = Vec::new();
    for (game, scenario) in scenarios {
        let values = runs
            .iter()
            .filter(|r| {
                r.entry.game == game
                    && r.entry.scenario == scenario
                    && r.entry.condition == manifest.baseline_condition
            })
            .filter_map(|r| r.stats.as_ref().map(|s| s.average_fps))
            .collect::<Vec<_>>();
        let baseline_mean = mean(&values);
        let cv = cv_percent(&values);
        gates.push(BaselineGate {
            game: game.clone(),
            scenario: scenario.clone(),
            n: values.len(),
            cv_percent: cv,
            limit_percent: gate_limit,
            pass: cv.is_some_and(|v| v <= gate_limit),
        });
        for run in runs
            .iter_mut()
            .filter(|r| r.entry.game == game && r.entry.scenario == scenario)
        {
            run.loss_percent = baseline_mean
                .zip(run.stats.as_ref())
                .map(|(base, s)| (base - s.average_fps) / base * 100.0);
        }
    }
    let mut groups: BTreeMap<(&str, &str, &str), Vec<&RunAnalysis>> = BTreeMap::new();
    for run in &runs {
        groups
            .entry((&run.entry.game, &run.entry.scenario, &run.entry.condition))
            .or_default()
            .push(run);
    }
    let mut conditions = Vec::new();
    for ((game, scenario, condition), group) in groups {
        let stats = group
            .iter()
            .filter_map(|r| r.stats.as_ref())
            .collect::<Vec<_>>();
        if stats.len() < 5 || stats.len() != manifest.runs_per_condition {
            warnings.push(format!("{game}/{scenario}/{condition}: n={} corridas válidas; protocolo pede 5; manifest pede {}.", stats.len(), manifest.runs_per_condition));
        }
        conditions.push(ConditionSummary {
            game: game.into(),
            scenario: scenario.into(),
            condition: condition.into(),
            n: stats.len(),
            fps: Estimate::from_values(&stats.iter().map(|s| s.average_fps).collect::<Vec<_>>()),
            loss_percent: Estimate::from_values(
                &group
                    .iter()
                    .filter_map(|r| r.loss_percent)
                    .collect::<Vec<_>>(),
            ),
            low_1_fps: Estimate::from_values(
                &stats.iter().map(|s| s.low_1_fps).collect::<Vec<_>>(),
            ),
            low_01_fps: Estimate::from_values(
                &stats.iter().map(|s| s.low_01_fps).collect::<Vec<_>>(),
            ),
            p99_frame_ms: Estimate::from_values(
                &stats.iter().map(|s| s.p99_frame_ms).collect::<Vec<_>>(),
            ),
            gpu_busy_ms: Estimate::from_values(
                &stats
                    .iter()
                    .filter_map(|s| s.mean_gpu_busy_ms)
                    .collect::<Vec<_>>(),
            ),
        });
    }
    if gates.iter().any(|g| g.n < 5) {
        warnings
            .push("Baseline incompleto: n < 5; PASS de CV isolado não fecha o milestone.".into());
    }
    if manifest
        .runs
        .iter()
        .map(|r| &r.game)
        .collect::<BTreeSet<_>>()
        .len()
        < 3
    {
        warnings.push("Menos de 3 jogos: cobertura insuficiente para aceite do M0.3.".into());
    }
    let gate_pass = !gates.is_empty() && gates.iter().all(|g| g.pass);
    Ok(Report {
        manifest,
        gates,
        gate_pass,
        conditions,
        runs,
        warnings,
    })
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\r', '\n'], " ")
}
fn number(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.3}"))
        .unwrap_or_else(|| "indisponível".into())
}
fn estimate(value: &Option<Estimate>) -> String {
    value
        .as_ref()
        .map(|e| format!("{:.3} ± {}", e.mean, number(e.ci95_half_width)))
        .unwrap_or_else(|| "indisponível".into())
}

pub fn markdown(report: &Report) -> String {
    let m = &report.manifest;
    let env = &m.environment;
    let environment = |v: &Option<String>| cell(v.as_deref().unwrap_or("indisponível"));
    let mut out = format!(
        "# Relatório de overhead — M0.3\n\nDados: **{}**. Criado em {}.\n\nGPU: {}  \nDriver: {}  \nPlano de energia: {}  \nWindows: {}  \nPresentMon: {}  \nSeed: {}  \nProtocolo: {} corridas, {} s, aquecimento {} s.\n\nGate de estabilidade do baseline: **{}**. O gate avalia somente CV, não o aceite completo do milestone.\n\n| Jogo | Cenário | n baseline | CV (%) | Limite (%) | Gate |\n|---|---|---:|---:|---:|---|\n",
        if m.synthetic {
            "SINTÉTICOS"
        } else {
            "capturados"
        },
        cell(&m.created_at),
        environment(&env.gpu),
        environment(&env.driver),
        environment(&env.power_plan),
        environment(&env.windows_version),
        environment(&env.presentmon_version),
        m.seed,
        m.runs_per_condition,
        m.duration_sec,
        m.warmup_sec,
        if report.gate_pass { "PASS" } else { "FAIL" }
    );
    for gate in &report.gates {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {:.3} | {} |\n",
            cell(&gate.game),
            cell(&gate.scenario),
            gate.n,
            number(gate.cv_percent),
            gate.limit_percent,
            if gate.pass { "PASS" } else { "FAIL" }
        ));
    }
    out.push_str("\nMédia ± meia largura do IC95 entre corridas (t de Student). GPUBusy em ms/frame; n pode variar quando a métrica está ausente (ver JSON).\n\n| Jogo | Cenário | Condição | n | FPS | Perda (%) | 1% low | 0,1% low | p99 (ms) | GPUBusy (ms) |\n|---|---|---|---:|---:|---:|---:|---:|---:|---:|\n");
    for c in &report.conditions {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            cell(&c.game),
            cell(&c.scenario),
            cell(&c.condition),
            c.n,
            estimate(&c.fps),
            estimate(&c.loss_percent),
            estimate(&c.low_1_fps),
            estimate(&c.low_01_fps),
            estimate(&c.p99_frame_ms),
            estimate(&c.gpu_busy_ms)
        ));
    }
    out.push_str("\n## Ordem das corridas\n\n| Ordem | ID | Jogo | Cenário | Rodada | Condição | Estado | CSV |\n|---:|---|---|---|---:|---|---|---|\n");
    let mut ordered = report.runs.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|r| r.entry.order);
    for r in ordered {
        let e = &r.entry;
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            e.order,
            cell(&e.id),
            cell(&e.game),
            cell(&e.scenario),
            e.round,
            cell(&e.condition),
            status_label(e.status),
            cell(e.csv.as_deref().unwrap_or("—"))
        ));
    }
    out.push_str("\n## Avisos\n\n");
    for warning in &report.warnings {
        out.push_str(&format!("- {}\n", cell(warning)));
    }
    for run in &report.runs {
        for warning in &run.warnings {
            out.push_str(&format!("- {}: {}\n", cell(&run.entry.id), cell(warning)));
        }
    }
    out.push_str("\nPerda por corrida usa o FPS médio das corridas do baseline do mesmo jogo/cenário; IC da perda considera esse baseline fixo. Frames e definições: consulte docs/bench/README.md. Relatório reproduzível com manifest e CSVs originais.\n");
    out
}

pub fn write(report: &Report, out: &Path) -> Result<(), Error> {
    fs::create_dir_all(out)?;
    serde_json::to_writer_pretty(File::create(out.join("summary.json"))?, report)?;
    fs::write(out.join("report.md"), markdown(report))?;
    let mut csv = csv::Writer::from_path(out.join("runs.csv"))?;
    csv.write_record([
        "id",
        "game",
        "scenario",
        "condition",
        "round",
        "order",
        "status",
        "frames",
        "duration_sec",
        "average_fps",
        "loss_percent",
        "low_1_fps",
        "low_01_fps",
        "p99_frame_ms",
        "mean_gpu_busy_ms",
        "invalid_rows",
        "invalid_optional_values",
        "warnings",
    ])?;
    for r in &report.runs {
        let e = &r.entry;
        let stat = |f: fn(&crate::stats::FrameStats) -> f64| {
            r.stats
                .as_ref()
                .map(|s| f(s).to_string())
                .unwrap_or_default()
        };
        csv.write_record([
            e.id.clone(),
            e.game.clone(),
            e.scenario.clone(),
            e.condition.clone(),
            e.round.to_string(),
            e.order.to_string(),
            format!("{:?}", e.status).to_lowercase(),
            r.stats
                .as_ref()
                .map(|s| s.frames.to_string())
                .unwrap_or_default(),
            stat(|s| s.duration_sec),
            stat(|s| s.average_fps),
            r.loss_percent.map(|v| v.to_string()).unwrap_or_default(),
            stat(|s| s.low_1_fps),
            stat(|s| s.low_01_fps),
            stat(|s| s.p99_frame_ms),
            r.stats
                .as_ref()
                .and_then(|s| s.mean_gpu_busy_ms)
                .map(|v| v.to_string())
                .unwrap_or_default(),
            r.invalid_rows.to_string(),
            r.invalid_optional_values.to_string(),
            r.warnings.join("; "),
        ])?;
    }
    csv.flush()?;
    Ok(())
}
