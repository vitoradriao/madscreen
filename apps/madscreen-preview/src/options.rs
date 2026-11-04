#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub cursor: bool,
    pub border: bool,
    pub diagnostics: bool,
    pub log: String,
}
impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> anyhow::Result<Self> {
        let mut options = Self {
            cursor: true,
            border: false,
            diagnostics: false,
            log: "info".into(),
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--no-cursor" => options.cursor = false,
                "--border" => options.border = true,
                "--diagnostics" => options.diagnostics = true,
                "--log" => {
                    options.log = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--log exige um nível"))?;
                    if !matches!(
                        options.log.as_str(),
                        "trace" | "debug" | "info" | "warn" | "error" | "off"
                    ) {
                        anyhow::bail!(
                            "Nível de log inválido: use trace, debug, info, warn, error ou off"
                        );
                    }
                }
                _ => anyhow::bail!(
                    "Opção desconhecida: {arg}. Use --no-cursor, --border, --diagnostics ou --log <nível>."
                ),
            }
        }
        Ok(options)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flags() -> anyhow::Result<()> {
        assert_eq!(
            Options::parse([])?,
            Options {
                cursor: true,
                border: false,
                diagnostics: false,
                log: "info".into()
            }
        );
        let parsed = Options::parse(
            ["--no-cursor", "--border", "--diagnostics", "--log", "debug"].map(String::from),
        )?;
        assert_eq!(
            parsed,
            Options {
                cursor: false,
                border: true,
                diagnostics: true,
                log: "debug".into()
            }
        );
        assert!(Options::parse(["--log".into()]).is_err());
        assert!(Options::parse(["--unknown".into()]).is_err());
        assert!(Options::parse(["--log".into(), "--border".into()]).is_err());
        assert!(Options::parse(["--log".into(), "verbose".into()]).is_err());
        Ok(())
    }
}
