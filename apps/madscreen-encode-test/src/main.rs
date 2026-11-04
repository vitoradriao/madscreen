#[cfg(windows)]
macro_rules! reportln {
    ($ipc:expr; $($args:tt)*) => {
        if $ipc {
            eprintln!($($args)*);
        } else {
            println!($($args)*);
        }
    };
}

#[cfg(any(windows, test))]
mod ipc;
#[cfg(windows)]
mod limits;
#[cfg(any(windows, test))]
mod options;
#[cfg(windows)]
mod picker;
#[cfg(all(windows, feature = "publish"))]
mod publish;
#[cfg(any(windows, test))]
mod report;
#[cfg(windows)]
mod runner;
#[cfg(windows)]
mod sources;

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "madscreen-encode-test requer Windows 10 1903+ x64, D3D11 e captura disponível. Use Windows para --probe e codificação."
    );
    std::process::exit(1);
}
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use clap::Parser;
    let mut options = options::Options::parse();
    options.height_choice = options.height;
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    if options.list_sources {
        let list = sources::list(options.exclude_pid, options.monitors_only)?;
        println!("{}", serde_json::to_string(&list)?);
        return Ok(());
    }
    if options.ipc {
        return ipc::run(options.report, options.frames_csv, options.capture_backend);
    }
    // Criar todo o pipeline na thread de encode; UI/telemetria nunca elevam prioridade.
    std::thread::Builder::new()
        .name("video-encode".into())
        .spawn(move || runner::run(options))?
        .join()
        .map_err(|_| anyhow::anyhow!("thread do encoder terminou inesperadamente"))?
}
