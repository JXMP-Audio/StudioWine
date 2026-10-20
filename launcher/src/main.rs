mod audio;
mod download;
mod fonts;
mod handlers;
mod icons;
mod log;
mod paths;
mod registry;
mod setup;
mod shortcuts;
mod ui;
mod state;
mod wine;

fn main() {
    std::process::exit(match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("studiowine: {e:#}");
            1
        }
    });
}

fn run() -> anyhow::Result<i32> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let mut paths = paths::Paths::resolve()?;
    if args.first().is_some_and(|a| a == "--wine") {
        // Menu entries and URL handlers name their environment (`env "WINEPREFIX=…"`),
        // which may be an earlier location than the one chosen now.
        if let Some(p) = std::env::var_os("WINEPREFIX").filter(|p| !p.is_empty()) {
            paths.prefix = p.into();
        }
        let status = wine::Wine::new(paths).command(&args[1..]).status()?;
        return Ok(status.code().unwrap_or(1));
    }
    ui::run_gui(paths)?;
    Ok(0)
}
