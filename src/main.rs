use clap::Parser;

mod complete;
mod config;
mod helpers;
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let config = config::load()?;

    print_motd(&cli.alias);

    if cli.watch {
        complete::cmd_watch(&config, &cli.alias, cli.pattern, cli.verbose).await
    } else {
        complete::cmd_complete(&config, &cli.alias, cli.pattern, cli.verbose).await
    }
}

fn print_motd(alias: &str) {
    let motd = include_str!("../MOTD.txt");
    println!("{motd}");
    if alias != "rik" {
        println!("(but call me {alias}!)\n");
    } else {
        println!();
    }
}
