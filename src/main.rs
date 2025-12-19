mod arguments;
mod git;
mod java_decompiler;
mod piston_meta;

use crate::arguments::Arguments;
use anyhow::Result;
use clap::Parser;
use log::{LevelFilter, info};
use obsidian_scheduler::callback::CallbackTimer;
use obsidian_scheduler::timer_trait::Timer;

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

    let timer = CallbackTimer::new(
        move |_| {
            let args_clone = args.clone();
            async move {
                run(&args_clone).await?;
                Ok(())
            }
        },
        tokio::time::Duration::from_hours(4),
    );

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl+C, shutting down...");
        }
        _ = async {
            while timer.is_running().await {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        } => {
            info!("Timer completed.");
        }
    }

    info!("Shutting down Minecraft Source Code Tracker...");
    Ok(())
}

async fn run(args: &Arguments) -> Result<()> {
    let args = args.clone();
    let temp_base = std::env::temp_dir().join("minecraft-sourcecode-tracker");
    let processing_directory = temp_base.join("working");
    let git_dir = temp_base.join("git");
    let processing_directory = processing_directory.to_string_lossy().to_string();
    let git_dir = git_dir.to_string_lossy().to_string();
    let processing_directory = processing_directory.as_str();
    let git_dir = git_dir.as_str();
    if tokio::fs::try_exists(git_dir).await? {
        tokio::fs::remove_dir_all(git_dir).await?;
    }
    tokio::fs::create_dir_all(git_dir).await?;
    if tokio::fs::try_exists(processing_directory).await? {
        tokio::fs::remove_dir_all(processing_directory).await?;
    }
    tokio::fs::create_dir_all(processing_directory).await?;

    let list_of_processed_versions = {
        let git_tracker = git::GitTracker::new(
            git_dir,
            processing_directory,
            &args.git_url,
            &args.git_username,
            &args.git_auth_token,
            args.git_email.clone(),
        )?;
        git_tracker.get_tags()?
    }; // git_tracker is dropped here, releasing file handles
    let versions_to_process = piston_meta::get_list_of_open_source_versions().await?;
    let mut versions_to_process = versions_to_process
        .iter()
        .filter(|version| !list_of_processed_versions.contains(&version.version))
        .collect::<Vec<_>>();
    versions_to_process.reverse();
    for version in versions_to_process {
        if tokio::fs::try_exists(git_dir).await? {
            tokio::fs::remove_dir_all(git_dir).await?;
        }
        tokio::fs::create_dir_all(git_dir).await?;
        let git_tracker = git::GitTracker::new(
            git_dir,
            processing_directory,
            &args.git_url,
            &args.git_username,
            &args.git_auth_token,
            args.git_email.clone(),
        )?;
        info!("Processing version {}", version.version);
        tokio::fs::create_dir_all(processing_directory).await?;
        let result = version.download().await?;
        java_decompiler::decompile_from_path(&result.client, processing_directory).await?;
        git_tracker.create_commit(
            processing_directory,
            format!(
                "Processed minecraft {} version {}",
                if version.is_snapshot {
                    "snapshot"
                } else {
                    "release"
                },
                version.version
            ),
            "client",
            Some(version.version.to_string()),
        )?;
        tokio::fs::remove_dir_all(processing_directory).await?;
        tokio::fs::remove_dir_all(git_dir).await?;
    }

    Ok(())
}
