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

    /// Model profile to use (e.g. "openrouter.gpt120")
    #[arg(long)]
    model: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let mut config = config::load(cli.model.as_deref())?;

    if cli.personality {
        config.personality = true;
    }

    let state = state::init_for_pattern(&cli.pattern, config)?;

    print_motd(&cli.alias, cli.model.as_deref(), &state.config);
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

fn print_motd(alias: &str, profile: Option<&str>, config: &config::Config) {
    if config.personality {
        personality::motd_personality();
    }
    println!("{}", format_motd(alias, profile, config));
}

fn format_motd(alias: &str, _profile: Option<&str>, config: &config::Config) -> String {
    let motd = include_str!("../MOTD.txt");
    let alias = if alias != "rik" {
        format!(" (call me \"{alias}\")\n")
    } else {
        String::new()
    };

    format!(
        "{}  {} / {}\n",
        motd.replace("{ALIAS}", &alias),
        config.model.provider,
        config.model.model,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_model_profile_flag() {
        let cli = Cli::try_parse_from(["rik", "--model", "openrouter.gpt120", "src"]).unwrap();

        assert_eq!(cli.model.as_deref(), Some("openrouter.gpt120"));
        assert_eq!(cli.pattern, "src");
    }

    #[test]
    fn motd_includes_current_model() {
        let mut config = config::Config::default();
        config.model.provider = config::Provider::OpenRouter;
        config.model.model = "gpt-120:turbo".to_owned();

        let motd = format_motd("rik", Some("openrouter.gpt120"), &config);

        assert!(motd.contains("OpenRouter / gpt-120:turbo"));
    }
}
