use crate::{
    Error,
    stats::{FrameStats, frame_stats},
};
use serde::Serialize;
use std::{
    collections::{BTreeSet, HashMap},
    io::Read,
};

#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    pub frame_ms: f64,
    pub gpu_busy_ms: Option<f64>,
    pub gpu_wait_ms: Option<f64>,
    pub cpu_busy_ms: Option<f64>,
    pub display_latency_ms: Option<f64>,
    pub displayed_time_ms: Option<f64>,
    pub application: Option<String>,
    pub present_mode: Option<String>,
    pub allows_tearing: Option<bool>,
}

#[derive(Debug, Default, Serialize)]
pub struct Capture {
    pub frames: Vec<Frame>,
    pub invalid_rows: usize,
    pub invalid_optional_values: usize,
    pub filtered_rows: usize,
    pub application_filter_unavailable: bool,
    pub initial_rows: usize,
    pub streams: usize,
    pub applications: BTreeSet<String>,
}

impl Capture {
    pub fn stats(&self) -> Option<FrameStats> {
        frame_stats(
            &self.frames.iter().map(|f| f.frame_ms).collect::<Vec<_>>(),
            &self
                .frames
                .iter()
                .filter_map(|f| f.gpu_busy_ms)
                .collect::<Vec<_>>(),
        )
    }
}

fn exe_name(value: &str) -> &str {
    value.rsplit(['/', '\\']).next().unwrap_or(value)
}

/// Deriva deltas por Application/ProcessID/SwapChainAddress. CPUStartTime usa ms;
/// TimeInSeconds usa segundos. A primeira linha de cada stream não tem delta.
pub fn parse(reader: impl Read, application: Option<&str>) -> Result<Capture, Error> {
    let mut csv = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(reader);
    let headers = csv.headers()?.clone();
    let column = |name: &str| {
        headers
            .iter()
            .position(|h| h.trim_start_matches('\u{feff}') == name)
    };
    let frame_col = column("FrameTime").or_else(|| column("MsBetweenPresents"));
    let time_col = column("CPUStartTime")
        .map(|i| (i, 1.0))
        .or_else(|| column("TimeInSeconds").map(|i| (i, 1000.0)));
    if frame_col.is_none() && time_col.is_none() {
        return Err(Error::Invalid(
            "CSV sem FrameTime, MsBetweenPresents ou timestamp suportado".into(),
        ));
    }
    let app_col = column("Application");
    let mut capture = Capture {
        application_filter_unavailable: application.is_some() && app_col.is_none(),
        ..Capture::default()
    };
    let mut previous: HashMap<(String, String, String), f64> = HashMap::new();
    let mut streams = BTreeSet::new();
    for row in csv.records() {
        let row = match row {
            Ok(row) => row,
            Err(_) => {
                capture.invalid_rows += 1;
                continue;
            }
        };
        let get = |index: Option<usize>| index.and_then(|i| row.get(i));
        let app = get(app_col).unwrap_or("");
        if let Some(filter) = application.filter(|_| app_col.is_some()) {
            if !exe_name(app).eq_ignore_ascii_case(exe_name(filter)) {
                capture.filtered_rows += 1;
                continue;
            }
        }
        capture.applications.insert(app.to_owned());
        let key = (
            app.to_lowercase(),
            get(column("ProcessID")).unwrap_or("").to_owned(),
            get(column("SwapChainAddress")).unwrap_or("").to_owned(),
        );
        streams.insert(key.clone());
        let frame_ms = if let Some(index) = frame_col {
            row.get(index).and_then(|s| s.parse::<f64>().ok())
        } else if let Some((index, scale)) = time_col {
            match row
                .get(index)
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.0)
            {
                Some(time) => {
                    let time = time * scale;
                    match previous.insert(key, time) {
                        Some(old) => Some(time - old),
                        None => {
                            capture.initial_rows += 1;
                            continue;
                        }
                    }
                }
                None => None,
            }
        } else {
            None
        };
        let frame_ms = match frame_ms.filter(|v| v.is_finite() && *v > 0.0) {
            Some(value) => value,
            None => {
                capture.invalid_rows += 1;
                continue;
            }
        };
        let mut optional = |name: &str| -> Option<f64> {
            let value = get(column(name))?;
            if value.is_empty()
                || value.eq_ignore_ascii_case("NA")
                || value.eq_ignore_ascii_case("N/A")
            {
                return None;
            }
            match value
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v >= 0.0)
            {
                Some(value) => Some(value),
                None => {
                    capture.invalid_optional_values += 1;
                    None
                }
            }
        };
        let gpu_busy_ms = optional("GPUBusy");
        let gpu_wait_ms = optional("GPUWait");
        let cpu_busy_ms = optional("CPUBusy");
        let display_latency_ms = optional("DisplayLatency");
        let displayed_time_ms = optional("DisplayedTime");
        let allows_tearing = match get(column("AllowsTearing")) {
            Some("1") | Some("true") => Some(true),
            Some("0") | Some("false") => Some(false),
            Some("") | Some("NA") | None => None,
            Some(_) => {
                capture.invalid_optional_values += 1;
                None
            }
        };
        capture.frames.push(Frame {
            frame_ms,
            gpu_busy_ms,
            gpu_wait_ms,
            cpu_busy_ms,
            display_latency_ms,
            displayed_time_ms,
            application: get(app_col).map(str::to_owned),
            present_mode: get(column("PresentMode")).map(str::to_owned),
            allows_tearing,
        });
    }
    capture.streams = streams.len();
    Ok(capture)
}
