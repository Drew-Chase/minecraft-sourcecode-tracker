use anyhow::{Result, anyhow};
use chrono::NaiveDateTime;
use log::{debug, info};
use std::env::temp_dir;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PistonMetaError {
    #[error("Failed to get the versions list from the piston version manifest v2")]
    FailedToGetVersionsList,
    #[error("version does not have a ID: {0}")]
    VersionDoesNotHaveID(String),
    #[error("'{0}' version does not have a release date")]
    VersionDoesNotHaveReleaseDate(String),
    #[error("'{0}' version does not have a manifest url")]
    VersionDoesNotHaveManifestUrl(String),
    #[error("'{0}' version does not have a client jar url")]
    VersionDoesNotHaveClientJar(String),
    #[error("'{0}' version does not have a server jar url")]
    VersionDoesNotHaveServerJar(String),
}

pub struct OpenSourceVersion {
    pub version: String,
    pub manifest_url: String,
    pub is_snapshot: bool,
    pub release_date: NaiveDateTime,
}

pub struct DownloadResult {
    server: PathBuf,
    client: PathBuf,
}

const MINECRAFT_PISTON_META_URL: &str =
    r#"https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"#;

/// This is the date of the first version that was deobfuscated: **26.1-snapshot-1**
const DATE_OF_OPEN_SOURCE: &str = "2025-12-16T12:42:29+00:00";

pub async fn get_list_of_open_source_versions() -> Result<Vec<OpenSourceVersion>> {
    info!("Getting list of open source versions");
    let manifest: serde_json::Value = reqwest::get(MINECRAFT_PISTON_META_URL)
        .await?
        .json()
        .await?;
    let mut open_source_versions: Vec<OpenSourceVersion> = Vec::new();
    let versions = manifest["versions"]
        .as_array()
        .ok_or(anyhow!(PistonMetaError::FailedToGetVersionsList))?;
    for version in versions {
        let id = version["id"]
            .as_str()
            .ok_or(anyhow!(PistonMetaError::VersionDoesNotHaveID(
                version.to_string()
            )))?;
        debug!("checking if version is open source: {}", id);
        let release_date = version["releaseTime"].as_str().ok_or(anyhow!(
            PistonMetaError::VersionDoesNotHaveReleaseDate(id.to_string())
        ))?;
        let manifest_url = version["url"].as_str().ok_or(anyhow!(
            PistonMetaError::VersionDoesNotHaveManifestUrl(id.to_string())
        ))?;
        let release_date = NaiveDateTime::parse_from_str(release_date, "%Y-%m-%dT%T%z")?; // 2025-12-16T12:42:29+00:00
        if is_date_open_source(release_date)? {
            let version = OpenSourceVersion {
                version: id.to_string(),
                manifest_url: manifest_url.to_string(),
                is_snapshot: version["type"].as_str().unwrap_or("release") == "snapshot",
                release_date,
            };
            info!("Found open source version: {}", id);
            open_source_versions.push(version);
        } else {
            // because the versions are sorted by release date,
            // if the current version is not open source,
            // then the rest of the versions will not be open source
            debug!("'{}' was not open source stopping the search.", id);
            break;
        }
    }

    Ok(open_source_versions)
}

pub async fn download_version(version: &OpenSourceVersion) -> Result<DownloadResult> {
    let time = chrono::Utc::now().timestamp();
    let client_path = temp_dir().join(format!("client-{}.jar", version.version));
    let server_path = temp_dir().join(format!("server-{}.jar", version.version));
    let client = reqwest::Client::new();

    let response = client.get(version.manifest_url.clone()).send().await?;
    let manifest: serde_json::Value = response.json().await?;

    let client_url = manifest["downloads"]["client"]["url"]
        .as_str()
        .ok_or(anyhow!(PistonMetaError::VersionDoesNotHaveClientJar(
            version.version.clone()
        )))?;
    let server_url = manifest["downloads"]["server"]["url"]
        .as_str()
        .ok_or(anyhow!(PistonMetaError::VersionDoesNotHaveServerJar(
            version.version.clone()
        )))?;

    let client_url = client_url.to_string();
    let server_url = server_url.to_string();
    let client_path_clone = client_path.clone();
    let server_path_clone = server_path.clone();

    let (client_result, server_result) = tokio::join!(
        async {
            info!("Downloading client jar");
            let response = client.get(&client_url).send().await?;
            let bytes = response.bytes().await?;
            tokio::fs::write(&client_path_clone, bytes).await?;
            info!("Client jar downloaded to {:?}", client_path_clone);
            Ok::<(), anyhow::Error>(())
        },
        async {
            info!("Downloading server jar");
            let response = client.get(&server_url).send().await?;
            let bytes = response.bytes().await?;
            tokio::fs::write(&server_path_clone, bytes).await?;
            info!("Server jar downloaded to {:?}", server_path_clone);
            Ok::<(), anyhow::Error>(())
        }
    );

    client_result?;
    server_result?;

    Ok(DownloadResult {
        client: client_path,
        server: server_path,
    })
}

/// This will check if the date provided is after the open source date
fn is_date_open_source(date: NaiveDateTime) -> Result<bool> {
    let date_of_open_source = NaiveDateTime::parse_from_str(DATE_OF_OPEN_SOURCE, "%Y-%m-%dT%T%z")?;
    Ok(date >= date_of_open_source)
}

mod test {
    #[tokio::test]
    async fn get_list_of_open_source_versions() {
        use log::LevelFilter;
        pretty_env_logger::env_logger::builder()
            .filter_level(LevelFilter::Trace)
            .is_test(false)
            .init();
        let versions = crate::piston_meta::get_list_of_open_source_versions()
            .await
            .unwrap();
        assert!(!versions.is_empty());
    }
    #[tokio::test]
    async fn download_version() {
        use log::LevelFilter;
        pretty_env_logger::env_logger::builder()
            .filter_level(LevelFilter::Trace)
            .is_test(false)
            .init();
        let versions = crate::piston_meta::get_list_of_open_source_versions()
            .await
            .unwrap()
            .pop()
            .unwrap();
        let result = crate::piston_meta::download_version(&versions)
            .await
            .unwrap();
        assert!(result.client.exists());
        assert!(result.server.exists());
    }
}
