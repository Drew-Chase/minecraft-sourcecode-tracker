mod arguments;
mod git;
mod java_decompiler;
mod piston_meta;

use crate::arguments::Arguments;
use anyhow::Result;
use clap::Parser;
use log::{LevelFilter, info};
use obsidian_scheduler::callback::CallbackTimer;

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::env_logger::builder()
        .filter_level(LevelFilter::Trace)
        .init();
    let args = Arguments::parse();
    info!(
        "Starting Minecraft Source Code Tracker v{}...",
        env!("CARGO_PKG_VERSION")
    );
    run(&args).await?;

    CallbackTimer::new(
        move |_| {
            let args_clone = args.clone();
            async move {
                run(&args_clone).await?;
                Ok(())
            }
        },
        tokio::time::Duration::from_hours(1),
    );

    Ok(())
}

async fn run(args: &Arguments) -> Result<()> {
    let args = args.clone();
    let processing_directory = "./working";
    tokio::fs::create_dir_all(processing_directory).await?;

    let git_tracker = git::GitTracker::new(
        processing_directory,
        args.git_url,
        args.git_username,
        args.git_auth_token,
        args.git_email,
    )?;
    let list_of_processed_versions = git_tracker.get_tags()?;
    let versions_to_process = piston_meta::get_list_of_open_source_versions().await?;
    let mut versions_to_process = versions_to_process
        .iter()
        .filter(|version| !list_of_processed_versions.contains(&version.version))
        .collect::<Vec<_>>();
    versions_to_process.reverse();
    for version in versions_to_process {
        info!("Processing version {}", version.version);
        let result = version.download().await?;
        java_decompiler::decompile_from_path(&result.client, processing_directory).await?;
        git_tracker.create_commit(format!("Processed minecraft {} version {}", if version.is_snapshot {"snapshot"}else{"release"}, version.version), "client", Some(version.version.to_string()))?;
        tokio::fs::remove_dir_all(processing_directory).await?;
        tokio::fs::create_dir_all(processing_directory).await?;
    }

    Ok(())
}
