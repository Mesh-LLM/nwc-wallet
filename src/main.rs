//! Standalone launcher; the wallet connects only through wallet.v1 RPC.
use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // The host passes the plugin's stderr through to its own log. RUST_LOG
    // overrides the default.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,mesh_wallet_nwc=info".into()),
        )
        .init();
    let name = std::env::var("MESH_LLM_PLUGIN_NAME")
        .context("Launch this executable through mesh-llm plugins, not directly")?;
    mesh_wallet_nwc::run_plugin(name, std::env::args().skip(1).collect()).await
}
