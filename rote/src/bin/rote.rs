use anyhow::Context as _;
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use rote_mux::Config;

const EXAMPLE_YAML: &str = include_str!("../../tests/data/example.yaml");

#[derive(Parser, Debug)]
#[command(author, version, about, args_conflicts_with_subcommands = true)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

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

#[derive(Subcommand, Debug)]
enum Command {
    /// Run rote with a configuration file
    Run(RunArgs),
    /// Run utility tools
    Tool(ToolArgs),
}

#[derive(Parser, Debug)]
struct RunArgs {
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

#[derive(Parser, Debug)]
struct ToolArgs {
    /// Wait and retry until the tool succeeds (exits with code 0)
    #[arg(long)]
    wait: bool,
    /// Interval between retries when --wait is specified (e.g., "1s", "500ms")
    #[arg(long, default_value = "1s", value_parser = parse_duration)]
    interval: Duration,
    #[command(subcommand)]
    tool: Tool,
}

#[derive(Subcommand, Debug)]
enum Tool {
    /// Check if a port is open on localhost
    IsPortOpen {
        /// The port number to check
        port: u16,
    },
    /// Make an HTTP GET request. Succeeds if the request completes (any status code).
    /// Accepts either a port number (assumes http://127.0.0.1:{port}/) or a full http(s) URL.
    HttpGet {
        /// Port number or full http(s) URL
        target: String,
    },
    /// Make an HTTP GET request and check for success (2xx status).
    /// Accepts either a port number (assumes http://127.0.0.1:{port}/) or a full http(s) URL.
    HttpGetOk {
        /// Port number or full http(s) URL
        target: String,
    },
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if let Some(ms) = s.strip_suffix("ms") {
        ms.parse::<u64>()
            .map(Duration::from_millis)
            .map_err(|e| format!("invalid milliseconds: {e}"))
    } else if let Some(secs) = s.strip_suffix('s') {
        secs.parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|e| format!("invalid seconds: {e}"))
    } else {
        s.parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|_| "expected duration like '1s' or '500ms'".to_string())
    }
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

    match args.command {
        Some(Command::Tool(tool_args)) => run_tool(tool_args).await,
        Some(Command::Run(run_args)) => run_main(run_args).await,
        None => {
            // Default behavior: use top-level args (backwards compatible)
            run_main(RunArgs {
                config: args.config,
                services: args.services,
                generate_example: args.generate_example,
            })
            .await
        }
    }
}

async fn run_main(args: RunArgs) -> anyhow::Result<()> {
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

async fn run_tool(args: ToolArgs) -> anyhow::Result<()> {
    use rote_mux::tools;

    loop {
        let result = match &args.tool {
            Tool::IsPortOpen { port } => tools::is_port_open(*port).await,
            Tool::HttpGet { target } => {
                // If it starts with http:// or https://, treat as URL; otherwise treat as port
                let url = if target.starts_with("http://") || target.starts_with("https://") {
                    target.clone()
                } else {
                    let port: u16 = target
                        .parse()
                        .map_err(|_| anyhow::anyhow!("invalid port number or URL: {}", target))?;
                    format!("http://127.0.0.1:{port}/")
                };
                tools::http_get(&url).await
            }
            Tool::HttpGetOk { target } => {
                // If it starts with http:// or https://, treat as URL; otherwise treat as port
                let url = if target.starts_with("http://") || target.starts_with("https://") {
                    target.clone()
                } else {
                    let port: u16 = target
                        .parse()
                        .map_err(|_| anyhow::anyhow!("invalid port number or URL: {}", target))?;
                    format!("http://127.0.0.1:{port}/")
                };
                tools::http_get_ok(&url).await
            }
        };

        match result {
            Ok(()) => return Ok(()),
            Err(e) => {
                if args.wait {
                    tokio::time::sleep(args.interval).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
}
