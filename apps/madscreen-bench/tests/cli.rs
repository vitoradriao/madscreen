#![allow(clippy::unwrap_used, clippy::expect_used)]
use madscreen_telemetry::model::{self, RunStatus};
use std::{fs, process::Command};

fn setup(fps: [f64; 5]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut m = model::example_manifest();
    m.environment.gpu = Some("GPU de teste".into());
    let prototype = m.runs[0].clone();
    m.runs.clear();
    for (index, fps) in fps.into_iter().enumerate() {
        let mut entry = prototype.clone();
        entry.id = format!("r{index}");
        entry.order = index + 1;
        entry.round = index + 1;
        entry.csv = Some(format!("r{index}.csv"));
        fs::write(
            dir.path().join(format!("r{index}.csv")),
            format!(
                "Application,FrameTime,GPUBusy\ngame.exe,{},2\n",
                1000.0 / fps
            ),
        )
        .unwrap();
        m.runs.push(entry);
    }
    let mut condition = prototype;
    condition.id = "c1".into();
    condition.order = 6;
    condition.condition = "obs-nvenc".into();
    condition.csv = Some("c1.csv".into());
    fs::write(
        dir.path().join("c1.csv"),
        "Application,FrameTime\ngame.exe,12.5\n",
    )
    .unwrap();
    m.runs.push(condition);
    fs::write(
        dir.path().join("manifest.json"),
        serde_json::to_vec(&m).unwrap(),
    )
    .unwrap();
    dir
}

fn analyze(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_madscreen-bench"))
        .arg("analyze")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn strict_pass_exports_loss_warnings_environment_and_order() {
    let dir = setup([100.0; 5]);
    let out = dir.path().join("out");
    let status = analyze(dir.path(), &["--strict", "--out", out.to_str().unwrap()]);
    assert!(status.status.success(), "{:?}", status);
    let json: model::Report =
        serde_json::from_slice(&fs::read(out.join("summary.json")).unwrap()).unwrap();
    assert!(json.gate_pass);
    assert_eq!(json.gates[0].n, 5);
    let condition = json
        .conditions
        .iter()
        .find(|c| c.condition == "obs-nvenc")
        .unwrap();
    assert!((condition.loss_percent.as_ref().unwrap().mean - 20.0).abs() < 1e-10);
    assert!(
        json.runs[0]
            .warnings
            .iter()
            .any(|w| w.contains("Duração medida"))
    );
    let report = fs::read_to_string(out.join("report.md")).unwrap();
    assert!(report.contains("GPU de teste"));
    assert!(report.contains("Ordem das corridas"));
    assert_eq!(
        fs::read_to_string(out.join("runs.csv"))
            .unwrap()
            .lines()
            .count(),
        7
    );
}

#[test]
fn strict_fail_writes_report_and_gate_limit_is_configurable() {
    let dir = setup([90.0, 95.0, 100.0, 105.0, 110.0]);
    assert!(!analyze(dir.path(), &["--strict"]).status.success());
    let json: model::Report =
        serde_json::from_slice(&fs::read(dir.path().join("summary.json")).unwrap()).unwrap();
    assert!(!json.gate_pass);
    assert!(!json.gates[0].pass);
    assert!(analyze(dir.path(), &[]).status.success());
    assert!(
        analyze(dir.path(), &["--strict", "--gate-cv", "10"])
            .status
            .success()
    );
}

#[test]
fn missing_baseline_and_skipped_runs_are_visible() {
    let dir = setup([100.0; 5]);
    let path = dir.path().join("manifest.json");
    let mut m: model::Manifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for r in &mut m.runs {
        if r.condition == "baseline" {
            r.status = RunStatus::Skipped;
            r.csv = None;
        }
    }
    fs::write(path, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(!analyze(dir.path(), &["--strict"]).status.success());
    let r: model::Report =
        serde_json::from_slice(&fs::read(dir.path().join("summary.json")).unwrap()).unwrap();
    assert_eq!(r.gates[0].n, 0);
    assert!(r.conditions.iter().all(|c| c.loss_percent.is_none()));
}

#[test]
fn schema_and_single_csv_work() {
    let schema = Command::new(env!("CARGO_BIN_EXE_madscreen-bench"))
        .arg("schema")
        .output()
        .unwrap();
    assert!(schema.status.success());
    let _: model::Manifest = serde_json::from_slice(&schema.stdout).unwrap();
    let dir = setup([100.0; 5]);
    let output = Command::new(env!("CARGO_BIN_EXE_madscreen-bench"))
        .arg("summarize-csv")
        .arg(dir.path().join("c1.csv"))
        .args(["--app", "game.exe"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["stats"]["average_fps"], 80.0);
}

#[test]
fn comparisons_use_only_the_baseline_of_the_same_game_and_scenario() {
    let dir = setup([100.0; 5]);
    let path = dir.path().join("manifest.json");
    let mut m: model::Manifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let original = m.runs.clone();
    for (i, mut entry) in original.into_iter().enumerate() {
        entry.id = format!("second-{i}");
        entry.order += 6;
        entry.scenario = "S-B".into();
        let ms = if entry.condition == "baseline" {
            5.0
        } else {
            10.0
        };
        entry.csv = Some(format!("second-{i}.csv"));
        fs::write(
            dir.path().join(entry.csv.as_ref().unwrap()),
            format!("Application,FrameTime\ngame.exe,{ms}\n"),
        )
        .unwrap();
        m.runs.push(entry);
    }
    fs::write(path, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(analyze(dir.path(), &["--strict"]).status.success());
    let r: model::Report =
        serde_json::from_slice(&fs::read(dir.path().join("summary.json")).unwrap()).unwrap();
    assert_eq!(r.gates.len(), 2);
    for (scenario, loss) in [("S-A", 20.0), ("S-B", 50.0)] {
        // O exemplo do schema usa S-A.
        let c = r
            .conditions
            .iter()
            .find(|c| c.condition == "obs-nvenc" && c.scenario == scenario)
            .unwrap();
        assert!((c.loss_percent.as_ref().unwrap().mean - loss).abs() < 1e-9);
    }
}

#[test]
fn invalid_manifest_and_unreadable_csv_do_not_silently_pass() {
    let dir = setup([100.0; 5]);
    assert!(!analyze(dir.path(), &["--gate-cv=NaN"]).status.success());
    assert!(!analyze(dir.path(), &["--gate-cv=-1"]).status.success());
    let path = dir.path().join("manifest.json");
    let mut m: model::Manifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for entry in &mut m.runs {
        entry.csv = Some("missing.csv".into());
    }
    fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(!analyze(dir.path(), &["--strict"]).status.success());
    let r: model::Report =
        serde_json::from_slice(&fs::read(dir.path().join("summary.json")).unwrap()).unwrap();
    assert!(
        r.runs
            .iter()
            .all(|r| r.warnings.iter().any(|w| w.contains("Falha ao analisar")))
    );
    m.runs[1].order = m.runs[0].order;
    fs::write(path, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(!analyze(dir.path(), &[]).status.success());
}
