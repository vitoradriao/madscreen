#![allow(clippy::unwrap_used, clippy::expect_used)]
use madscreen_telemetry::{presentmon, stats::*};

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

#[test]
fn student_ci_and_cv_independently_checked_with_python_statistics() {
    // python3: statistics.stdev([98,99,100,101,102]) = sqrt(2.5);
    // CV = 1.58113883008419%; 2.776*sd/sqrt(5) = 1.9629284245738559.
    let x = [98.0, 99.0, 100.0, 101.0, 102.0];
    let estimate = Estimate::from_values(&x).unwrap();
    near(estimate.mean, 100.0);
    near(estimate.std_dev.unwrap(), 1.5811388300841898);
    near(estimate.ci95_half_width.unwrap(), 1.9629284245738559);
    near(cv_percent(&x).unwrap(), 1.58113883008419);
    assert_eq!(t_critical(0), None);
    assert_eq!(t_critical(1), Some(12.706));
    assert_eq!(t_critical(30), Some(2.042));
    assert_eq!(t_critical(31), Some(1.96));
}

#[test]
fn distributions_and_count_based_lows() {
    let times = [10.0, 20.0, 30.0];
    let stats = frame_stats(&times, &[4.0, 6.0, 8.0]).unwrap();
    near(stats.average_fps, 50.0); // 3 / 0.060
    near(stats.duration_sec, 0.060);
    near(stats.p99_frame_ms, 29.8); // índice 1.98
    near(stats.low_1_fps, 1000.0 / 30.0);
    near(stats.low_01_fps, 1000.0 / 30.0);
    near(stats.mean_gpu_busy_ms.unwrap(), 6.0);
    let mut many = vec![10.0; 198];
    many.extend([20.0, 40.0]);
    near(low_fps(&many, 0.01).unwrap(), 1000.0 / 30.0);
    assert_eq!(percentile(&times, 0.0), Some(10.0));
    assert_eq!(percentile(&times, 1.0), Some(30.0));
}

#[test]
fn empty_single_and_nonfinite_statistics() {
    assert!(mean(&[]).is_none());
    assert!(sample_std_dev(&[1.0]).is_none());
    assert!(cv_percent(&[0.0, 0.0]).is_none());
    assert!(mean(&[f64::NAN]).is_none());
    assert!(percentile(&[1.0], f64::NAN).is_none());
    assert!(frame_stats(&[0.0], &[]).is_none());
    assert!(frame_stats(&[f64::INFINITY], &[]).is_none());
    let estimate = Estimate::from_values(&[1.0]).unwrap();
    assert!(estimate.ci95_half_width.is_none());
    assert!(
        frame_stats(&[10.0], &[])
            .unwrap()
            .mean_gpu_busy_ms
            .is_none()
    );
}

#[test]
fn recent_and_legacy_formats_agree() {
    for input in [
        include_str!("fixtures/v2_frame.csv"),
        include_str!("fixtures/v1.csv"),
    ] {
        let capture = presentmon::parse(input.as_bytes(), Some("game.exe")).unwrap();
        assert_eq!(capture.invalid_rows, 0);
        near(capture.stats().unwrap().average_fps, 50.0);
    }
    let c = presentmon::parse(include_bytes!("fixtures/v2_frame.csv").as_slice(), None).unwrap();
    assert_eq!(c.frames[0].allows_tearing, Some(true));
    assert_eq!(c.frames[1].allows_tearing, Some(false));
    assert_eq!(c.frames[0].gpu_wait_ms, Some(1.0));
    assert_eq!(c.frames[0].cpu_busy_ms, Some(2.0));
    assert_eq!(c.frames[2].displayed_time_ms, Some(0.0));
    assert_eq!(c.frames[0].display_latency_ms, Some(8.0));
    assert_eq!(
        c.frames[1].present_mode.as_deref(),
        Some("Hardware: Independent Flip")
    );
}

#[test]
fn cpu_start_time_is_milliseconds_and_initial_row_is_not_a_frame() {
    let c = presentmon::parse(include_bytes!("fixtures/v2_0.csv").as_slice(), None).unwrap();
    assert_eq!(c.initial_rows, 1);
    assert_eq!(c.frames.len(), 2);
    near(c.frames[0].frame_ms, 10.0);
    near(c.frames[1].frame_ms, 20.0);
    near(c.stats().unwrap().average_fps, 200.0 / 3.0);
    let c = presentmon::parse("TimeInSeconds\n1\n1.01\n1.03\n".as_bytes(), None).unwrap();
    near(c.stats().unwrap().average_fps, 200.0 / 3.0);
}

#[test]
fn malformed_rows_are_counted_and_optional_na_is_allowed() {
    let c = presentmon::parse(include_bytes!("fixtures/invalid.csv").as_slice(), None).unwrap();
    assert_eq!(c.invalid_rows, 6);
    assert_eq!(c.invalid_optional_values, 1);
    assert_eq!(c.frames.len(), 3);
    near(c.stats().unwrap().average_fps, 50.0);
    near(c.stats().unwrap().mean_gpu_busy_ms.unwrap(), 5.0);
    assert!(presentmon::parse("CPUBusy\n2\n".as_bytes(), None).is_err());
    let c = presentmon::parse("FrameTime\n10\n".as_bytes(), Some("a.exe")).unwrap();
    assert!(c.application_filter_unavailable);
    assert_eq!(c.frames.len(), 1);
}

#[test]
fn filter_is_case_insensitive_and_deltas_are_per_stream() {
    let c = presentmon::parse(
        include_bytes!("fixtures/multi.csv").as_slice(),
        Some("C:\\Games\\game.exe"),
    )
    .unwrap();
    assert_eq!(c.filtered_rows, 2);
    assert_eq!(c.initial_rows, 2);
    assert_eq!(c.streams, 2);
    assert_eq!(c.frames.len(), 2);
    near(c.frames[0].frame_ms, 10.0);
    near(c.frames[1].frame_ms, 20.0);
    let all = presentmon::parse(include_bytes!("fixtures/multi.csv").as_slice(), None).unwrap();
    assert_eq!(all.streams, 3);
    assert_eq!(all.frames.len(), 3);
}

#[test]
fn bom_and_preferred_frame_column() {
    let c = presentmon::parse(
        "\u{feff}FrameTime,MsBetweenPresents,CPUStartTime\n10,100,0\n20,200,100\n".as_bytes(),
        None,
    )
    .unwrap();
    near(c.stats().unwrap().average_fps, 200.0 / 3.0);
}
