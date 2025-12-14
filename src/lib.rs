mod config;
mod process;

use std::process::{Command, Stdio};

pub use config::{Config, ServiceAction, ServiceConfiguration};
pub use process::{Process, ProcessState};
