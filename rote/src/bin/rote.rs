use anyhow::Context as _;
use clap::Parser;
use std::fs;
use std::path::PathBuf;

use rote_mux::Config;

const EXAMPLE_YAML: &str = include_str!("../../tests/data/example.yaml");

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// The path to the configuration file. If omitted will look for `rote.yaml`
    /// in the current directory.
    #[arg(short, long, value_name = "FILE")]
    config: Option<String>,
    /// The services to run. If omitted, the default service from the config
    /// file will be run. If the default service is not specified in the config,
    /// no services will be run.
    #[arg(value_name = "SERVICE", required = false)]
    services: Vec<String>,
    /// Print an example configuration file to stdout and exit.
    #[arg(long)]
    generate_example: bool,
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e}");
        for cause in e.chain().skip(1) {
            eprintln!("  caused by: {cause}");
        }
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let args = Args::parse();

    if args.generate_example {
        println!("{EXAMPLE_YAML}");
        return Ok(());
    }

    let config_path = if let Some(config) = args.config {
        PathBuf::from(config)
    } else {
        PathBuf::from("rote.yaml")
    };

    let yaml_dir = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("failed to determine config file directory"))?
        .to_path_buf();

    let yaml_str = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read config file '{}'", config_path.display()))?;

    let config: Config =
        serde_yaml::from_str(&yaml_str).context("failed to parse config file as YAML")?;

    rote_mux::run(config, args.services, yaml_dir).await?;

    Ok(())
}
