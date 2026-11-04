use crate::stats::{Estimate, FrameStats};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub created_at: String,
    pub synthetic: bool,
    pub seed: i32,
    pub runs_per_condition: usize,
    pub duration_sec: f64,
    pub warmup_sec: f64,
    pub baseline_condition: String,
    pub environment: Environment,
    pub runs: Vec<RunEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Environment {
    pub gpu: Option<String>,
    pub driver: Option<String>,
    pub power_plan: Option<String>,
    pub windows_version: Option<String>,
    pub presentmon_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Complete,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEntry {
    pub id: String,
    pub game: String,
    pub scenario: String,
    pub condition: String,
    pub round: usize,
    pub order: usize,
    pub application: String,
    pub status: RunStatus,
    pub csv: Option<String>,
    pub duration_sec: f64,
    pub warmup_sec: f64,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub gpu_before: Option<serde_json::Value>,
    #[serde(default)]
    pub gpu_after: Option<serde_json::Value>,
    #[serde(default)]
    pub gpu_samples_csv: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAnalysis {
    pub entry: RunEntry,
    pub stats: Option<FrameStats>,
    pub loss_percent: Option<f64>,
    pub invalid_rows: usize,
    pub invalid_optional_values: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionSummary {
    pub game: String,
    pub scenario: String,
    pub condition: String,
    pub n: usize,
    pub fps: Option<Estimate>,
    pub loss_percent: Option<Estimate>,
    pub low_1_fps: Option<Estimate>,
    pub low_01_fps: Option<Estimate>,
    pub p99_frame_ms: Option<Estimate>,
    pub gpu_busy_ms: Option<Estimate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineGate {
    pub game: String,
    pub scenario: String,
    pub n: usize,
    pub cv_percent: Option<f64>,
    pub limit_percent: f64,
    pub pass: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub manifest: Manifest,
    pub gates: Vec<BaselineGate>,
    pub gate_pass: bool,
    pub conditions: Vec<ConditionSummary>,
    pub runs: Vec<RunAnalysis>,
    pub warnings: Vec<String>,
}

pub fn example_manifest() -> Manifest {
    Manifest {
        schema_version: 1,
        created_at: "2026-10-29T12:00:00Z".into(),
        synthetic: false,
        seed: 42,
        runs_per_condition: 5,
        duration_sec: 180.0,
        warmup_sec: 60.0,
        baseline_condition: "baseline".into(),
        environment: Environment::default(),
        runs: vec![RunEntry {
            id: "run-0001".into(),
            game: "Jogo DX11".into(),
            scenario: "S-A".into(),
            condition: "baseline".into(),
            round: 1,
            order: 1,
            application: "game.exe".into(),
            status: RunStatus::Complete,
            csv: Some("run-0001.csv".into()),
            duration_sec: 180.0,
            warmup_sec: 60.0,
            notes: None,
            gpu_before: None,
            gpu_after: None,
            gpu_samples_csv: None,
        }],
    }
}
