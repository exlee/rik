use clap::Parser;

mod cleanup;
mod complete;
mod config;
mod helpers;
mod keyboard;
mod markers;
mod personality;
mod raii;
mod state;
mod tools;

#[derive(Parser)]
#[command(name = "rik", about = "Complete '<alias>: <query>' markers in files")]
struct Cli {
    /// File path or glob pattern to scan; multiple patterns can be joined with ","
    /// (e.g. "src/**/*.rs,tests/**/*.rs")
    pattern: String,

    /// Watch for changes and complete markers continuously
    #[arg(short, long)]
    watch: bool,

    /// Marker alias prefix (default: "rik")
    #[arg(short, long, default_value = "rik")]
    alias: String,

    /// Print agent details alongside completion
    #[arg(short, long)]
    verbose: bool,

    /// Enable personality
    #[arg(long)]
    personality: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let mut config = config::load()?;

    if cli.personality {
        config.personality = true;
    }

    let state = state::init_for_pattern(&cli.pattern, config)?;

    print_motd(&cli.alias, &state.config);
    let _ = ctrlc::set_handler(|| {
        cleanup::cleanup();
        std::process::exit(0);
    });

    if cli.watch {
        crate::keyboard::start_escape_listener();
        complete::cmd_watch(state, &cli.alias, cli.pattern, cli.verbose).await
    } else {
        complete::cmd_complete(state, &cli.alias, cli.pattern, cli.verbose).await
    }
}

fn print_motd(alias: &str, config: &config::Config) {
    let motd = include_str!("../MOTD.txt");
    let alias = if alias != "rik" {
        format!(" (call me \"{alias}\")\n")
    } else {
        String::new()
    };
    if config.personality {
        personality::motd_personality();
    }
    println!("{}", motd.replace("{ALIAS}", &alias));
}
