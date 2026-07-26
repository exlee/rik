use clap::Parser;

mod cleanup;
mod complete;
mod config;
mod helpers;
mod keyboard;
mod markers;
mod personality;
mod raii;
mod skills;
mod state;
mod tools;
mod watchdog;

#[derive(Parser)]
#[command(name = "rik", about = "Complete '<alias>: <query>' markers in files")]
struct Cli {
    /// File path or glob pattern to scan; multiple patterns can be joined with ","
    /// (e.g. "src/**/*.rs,tests/**/*.rs")
    pattern: String,

    /// Complete markers once, then exit
    #[arg(short = '1', long)]
    once: bool,

    /// Marker alias prefix (default: "rik")
    #[arg(short, long, default_value = "rik")]
    alias: String,

    /// Print agent details alongside completion
    #[arg(short, long)]
    verbose: bool,

    /// Enable personality
    #[arg(long)]
    personality: bool,

    /// Replace question markers with Q/A blocks
    #[arg(long)]
    write_answers: bool,

    /// Model profile to use (e.g. "openrouter.gpt120")
    #[arg(long)]
    model: Option<String>,

    /// Additional overarching instructions for the agent
    #[arg(short = 's', long)]
    system_prompt: Option<String>,

    /// Do not honor .gitignore, .ignore, or git exclude files when scanning
    #[arg(long)]
    no_ignore: bool,

    /// Preload skills by name, so the agent starts with their full instructions
    /// (comma-separated, or repeat the flag)
    #[arg(long, value_delimiter = ',', value_name = "NAME")]
    skills: Vec<String>,
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
    if cli.write_answers {
        config.write_answers = true;
    }

    let all_skills = skills::all();
    let preloaded = skills::resolve_requested(all_skills, &cli.skills)?;
    let skill_section = skills::prompt_section(all_skills, &preloaded)?;

    let state = state::init_for_pattern(&cli.pattern, config)?;

    print_motd(&cli.alias, cli.model.as_deref(), &state.config);
    if !preloaded.is_empty() {
        println!(
            "Preloaded skill(s): {}\n",
            preloaded
                .iter()
                .map(|skill| format!("{} ({})", skill.name, skill.source.label()))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let _ = ctrlc::set_handler(|| {
        cleanup::cleanup();
        std::process::exit(0);
    });

    if !cli.once {
        crate::keyboard::start_escape_listener();
        complete::cmd_watch(
            state,
            &cli.alias,
            cli.pattern,
            cli.verbose,
            cli.no_ignore,
            cli.system_prompt.as_deref(),
            &skill_section,
        )
        .await
    } else {
        complete::cmd_complete(
            state,
            &cli.alias,
            cli.pattern,
            cli.verbose,
            cli.no_ignore,
            cli.system_prompt.as_deref(),
            &skill_section,
        )
        .await
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
    fn watches_by_default_and_supports_once_flags() {
        let default = Cli::try_parse_from(["rik", "src"]).unwrap();
        let long = Cli::try_parse_from(["rik", "--once", "src"]).unwrap();
        let short = Cli::try_parse_from(["rik", "-1", "src"]).unwrap();

        assert!(!default.once);
        assert!(long.once);
        assert!(short.once);
    }

    #[test]
    fn parses_system_prompt_flags() {
        let long =
            Cli::try_parse_from(["rik", "--system-prompt", "Respond in jokes", "src"]).unwrap();
        let short = Cli::try_parse_from(["rik", "-s", "Write Rust tests", "src"]).unwrap();

        assert_eq!(long.system_prompt.as_deref(), Some("Respond in jokes"));
        assert_eq!(short.system_prompt.as_deref(), Some("Write Rust tests"));
    }

    #[test]
    fn parses_no_ignore_flag() {
        let cli = Cli::try_parse_from(["rik", "--no-ignore", "src"]).unwrap();

        assert!(cli.no_ignore);
        assert_eq!(cli.pattern, "src");
    }

    #[test]
    fn parses_write_answers_flag() {
        let cli = Cli::try_parse_from(["rik", "--write-answers", "src"]).unwrap();

        assert!(cli.write_answers);
        assert_eq!(cli.pattern, "src");
    }

    #[test]
    fn parses_skills_flag_as_comma_separated_or_repeated() {
        let joined = Cli::try_parse_from(["rik", "--skills", "alpha,beta", "src"]).unwrap();
        let repeated =
            Cli::try_parse_from(["rik", "--skills", "alpha", "--skills", "beta", "src"]).unwrap();
        let absent = Cli::try_parse_from(["rik", "src"]).unwrap();

        assert_eq!(joined.skills, ["alpha", "beta"]);
        assert_eq!(repeated.skills, ["alpha", "beta"]);
        assert!(absent.skills.is_empty());
    }

    #[test]
    fn help_lists_system_prompt_flag() {
        use clap::CommandFactory as _;

        let help = Cli::command().render_help().to_string();

        assert!(help.contains("-s, --system-prompt <SYSTEM_PROMPT>"));
        assert!(help.contains("-1, --once"));
        assert!(help.contains("--no-ignore"));
        assert!(help.contains("--write-answers"));
        assert!(help.contains("--skills <NAME>"));
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
