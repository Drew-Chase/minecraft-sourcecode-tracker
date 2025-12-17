use anyhow::Result;
use log::*;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use thiserror::Error;
use which::which;

const CFR_BYTES: &[u8] = include_bytes!("../vendors/cfr-0.152.jar");

#[derive(Error, Debug)]
pub enum JavaDecompilerError {
    #[error("Unable to detect an installed version of java")]
    NoJava,
    #[error("Failed to extract cfr decompiler jar to the temp directory")]
    CFRExtractionFailure,
    #[error(
        "Unable to decompile the java source code, process exited with status code {1}\nThe command executed: '{0}'"
    )]
    DecompilationFailure(String, i32),
}

pub async fn decompile_from_path(path: impl AsRef<Path>, output: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    let output = output.as_ref();
    info!("Decompiling path {}", path.display());
    if let Ok(java_path) = which("java") {
        if let Ok(cfr_path) = extract_cfr() {
            let java_path = java_path.to_string_lossy().to_string();
            let cfr_path = cfr_path.to_string_lossy().to_string();
            let output_path = output.to_string_lossy().to_string();
            let input_path = path.to_string_lossy().to_string();
            let args = [
                "-jar",
                cfr_path.as_str(),
                input_path.as_str(),
                "--outputdir",
                output_path.as_str(),
            ];
            let command = format!("{} {}", java_path, args.join(" "));
            let mut child = Command::new(java_path)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            while let Some(stderr) = child.stderr.take() {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    info!("{}", line?);
                }
            }
            debug!("Finished Decompiling path {}", path.display());
            let exit_status = child.wait()?;
            if !exit_status.success() {
                return Err(JavaDecompilerError::DecompilationFailure(
                    command,
                    exit_status.code().unwrap_or(-1),
                )
                .into());
            }
        } else {
            return Err(JavaDecompilerError::CFRExtractionFailure.into());
        }
    } else {
        return Err(JavaDecompilerError::NoJava.into());
    }

    Ok(())
}

fn extract_cfr() -> Result<PathBuf> {
    debug!("Extracting cfr from memory");
    let cfr_path = std::env::temp_dir().join("cfr.jar");
    std::fs::write(&cfr_path, CFR_BYTES)?;
    if std::fs::exists(&cfr_path)? {
        debug!("Extracted cfr to {:?}", cfr_path);
        Ok(cfr_path)
    } else {
        error!("Failed to extract cfr from memory");
        Err(JavaDecompilerError::CFRExtractionFailure.into())
    }
}

mod test {
    #[tokio::test]
    async fn decompile_client_jar() {
        use crate::java_decompiler::decompile_from_path;
        use log::LevelFilter;
        use std::path::PathBuf;

        pretty_env_logger::env_logger::builder()
            .filter_level(LevelFilter::Trace)
            .is_test(false)
            .init();
        let path = PathBuf::from("./vendors/client.jar");
        let output = "./target/tests/decompile_client_jar/";
        decompile_from_path(path, output).await.unwrap();
    }
}
