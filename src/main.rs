use clap::Parser;
use std::path::PathBuf;
use tephra::config::Config;

#[derive(Parser)]
#[command(version, about = "tephra: a small self-hosted server for TRMNL devices")]
struct Args {
    /// TOML configuration file (defaults are used when omitted).
    #[arg(short, long)]
    config: Option<PathBuf>,
    /// Print the effective configuration as TOML and exit.
    #[arg(long)]
    print_config: bool,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    let cfg = match &args.config {
        Some(path) => Config::load(path)?,
        None => Config::default(),
    };
    if args.print_config {
        print!("{}", toml::to_string_pretty(&cfg)?);
        return Ok(());
    }
    if !cfg.timezone.is_empty() {
        // Apps read local time through Lua's os.date, which follows TZ.
        // SAFETY: no other threads exist yet; the runtime starts below.
        unsafe { std::env::set_var("TZ", &cfg.timezone) };
    }
    tokio::runtime::Runtime::new()?.block_on(tephra::run(cfg))
}
