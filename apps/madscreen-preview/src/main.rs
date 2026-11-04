mod options;
#[cfg(windows)]
mod windows;

fn main() -> anyhow::Result<()> {
    let options = options::Options::parse(std::env::args().skip(1))?;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| tracing_subscriber::EnvFilter::try_new(&options.log))?;
    tracing_subscriber::fmt().with_env_filter(filter).init();
    #[cfg(windows)]
    {
        windows::run(options)
    }
    #[cfg(not(windows))]
    {
        eprintln!("O madscreen-preview só roda no Windows 10 2004+ ou Windows 11.");
        std::process::exit(1);
    }
}
