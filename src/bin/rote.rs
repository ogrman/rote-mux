use clap::Parser;
use rote::config::Config;
use rote::execute_service;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short, long, value_name = "FILE")]
    config: String,
    #[arg(value_name = "SERVICE", required = false)]
    services: Vec<String>,
}

fn main() {
    let args = Args::parse();
    let config_path = PathBuf::from(&args.config);
    let yaml_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let yaml_str = fs::read_to_string(&config_path).expect("Failed to read config file");
    let config: Config = serde_yaml::from_str(&yaml_str).expect("Failed to parse config");

    let services_to_run = if args.services.is_empty() {
        vec![config.default.clone()]
    } else {
        args.services.clone()
    };

    for name in services_to_run {
        if let Some(service) = config.services.get(&name) {
            execute_service(service, yaml_dir);
        } else {
            eprintln!("Service '{}' not found in config", name);
        }
    }
}
