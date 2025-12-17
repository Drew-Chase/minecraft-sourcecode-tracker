mod java_decompiler;
mod piston_meta;

use anyhow::Result;
use log::LevelFilter;

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::env_logger::builder()
        .filter_level(LevelFilter::Trace)
        .init();
    Ok(())
}
