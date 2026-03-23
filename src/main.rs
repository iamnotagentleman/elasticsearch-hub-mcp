mod config;
mod connection_manager;
mod docs;
mod memory;
mod query_rules;
mod server;

use anyhow::Result;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use std::path::PathBuf;
use tracing_subscriber::{self, EnvFilter};

use crate::config::load_config;
use crate::connection_manager::ConnectionManager;
use crate::server::ElasticsearchMcpServer;

#[tokio::main]
async fn main() -> Result<()> {
    // Log to stderr (stdout is the MCP transport)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting Elasticsearch Hub MCP server (Rust)");

    // Determine project root: ES_MCP_PROJECT_ROOT > ~/.elasticsearch-hub-mcp
    let project_root = std::env::var("ES_MCP_PROJECT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .map(|h| h.join(".elasticsearch-hub-mcp"))
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        });

    // Create required directories
    std::fs::create_dir_all(project_root.join("memories")).ok();
    std::fs::create_dir_all(project_root.join(".tmp")).ok();

    // Load config
    let instances = load_config(None)?;
    tracing::info!("Loaded {} ES instances", instances.len());

    // Initialize connection manager
    let cm = ConnectionManager::new(instances)?;

    // Create MCP server and serve on stdio
    let mcp_server = ElasticsearchMcpServer::new(cm, project_root);
    let service = mcp_server
        .serve(stdio())
        .await
        .inspect_err(|e| {
            tracing::error!("serving error: {:?}", e);
        })?;

    tracing::info!("MCP server running on stdio");
    service.waiting().await?;
    Ok(())
}
