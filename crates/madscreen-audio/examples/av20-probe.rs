//! Isolated generated-process packet timing. No system-wide capture, no mixer.
#[cfg(windows)]
mod probe_sys {
    #![allow(unsafe_code)]
    use anyhow::{Context, Result, bail};
    use std::{
        io::{BufRead, Write},
        time::Duration,
    };
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

    pub fn main() -> Result<()> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.len() != 4 || args[0] != "--generated-pid" {
            bail!("av20-probe --generated-pid PID SECONDS OUTPUT.json");
        }
        let pid: u32 = args[1].parse()?;
        let seconds: u64 = args[2].parse()?;
        if pid == 0 || !(1..=600).contains(&seconds) {
            bail!("invalid pid/duration");
        }
        let output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[3])?;
        // SAFETY: valid pointers to initialized stack integers; QPC is process independent.
        let frequency = unsafe {
            let mut value = 0;
            QueryPerformanceFrequency(&mut value)?;
            value
        };
        if frequency <= 0 {
            bail!("invalid QPC frequency");
        }
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                if !matches!(line.as_deref(), Ok("clock")) {
                    break;
                }
                // SAFETY: the stack integer lives through the synchronous API call.
                let stamp = unsafe {
                    let mut value = 0;
                    if QueryPerformanceCounter(&mut value).is_err() {
                        break;
                    }
                    value
                };
                let converted = i128::from(stamp) * 10_000_000 / i128::from(frequency);
                println!("{{\"qpc100ns\":{converted}}}");
                let _ = std::io::stdout().flush();
            }
        });
        println!("{{\"ready\":true,\"qpcFrequency\":{frequency}}}");
        std::io::stdout().flush()?;
        let report =
            madscreen_audio::diagnostic::probe_process_packets(pid, Duration::from_secs(seconds))?;
        let mut output = std::io::BufWriter::new(output);
        serde_json::to_writer(&mut output, &report).context("writing packet report")?;
        output.flush()?;
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    probe_sys::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows-only packet probe");
}
