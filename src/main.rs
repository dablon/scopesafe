mod auditor;
mod cli;
mod db;
mod error;
mod mcp;
mod patterns;
mod report;
mod scope;
mod tracker;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        if let Some(scopesafe_err) = e.downcast_ref::<error::Error>() {
            return ExitCode::from(scopesafe_err.exit_code() as u8);
        }
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run() -> Result<()> {
    let cli = cli::Cli::parse();
    cli.execute()
}

/// Entry point for the MCP server (used by `scopesafe mcp`).
pub fn run_mcp(project_root: PathBuf) -> Result<()> {
    let server = mcp::McpServer::new(project_root)?;
    server.run()
}
