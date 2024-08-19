use crate::fs::workspace_directory;
use crate::io_models::context::Context;
use crate::io_models::engine_request::Archive;
use crate::log_file_writer::LogFileWriter;
use crate::models::abort::Abort;
use reqwest::header::CONTENT_TYPE;
use std::path::Path;
use std::time::Duration;
use tokio::sync::broadcast;

pub mod environment_task;
pub mod infrastructure_task;
pub mod qovery_api;

pub trait Task: Send + Sync {
    fn id(&self) -> &str;
    fn run(&self);
    fn cancel(&self, force_requested: bool) -> bool;
    fn cancel_checker(&self) -> Box<dyn Abort>;
    fn is_terminated(&self) -> bool;
    fn await_terminated(&self) -> broadcast::Receiver<()>;
}

fn upload_s3_file(archive: Option<&Archive>, file_path: &Path) -> Result<(), anyhow::Error> {
    let archive = match archive {
        Some(archive) => archive,
        None => {
            info!("no archive upload (request.archive is None)");
            return Ok(());
        }
    };

    info!(
        "Sending file {} to bucket {}://{}{}",
        file_path.to_str().unwrap_or_default(),
        archive.upload_url.scheme(),
        archive.upload_url.host_str().unwrap_or(""),
        archive.upload_url.path()
    );

    let file = std::fs::File::open(file_path)?;
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()?
        .put(archive.upload_url.clone())
        .header(CONTENT_TYPE, "application/octet-stream")
        .body(file)
        .timeout(Duration::from_secs(60 * 5))
        .send()?
        .error_for_status()?;

    Ok(())
}

fn enable_log_file_writer(context: &Context, log_file_writer: &Option<LogFileWriter>) {
    if let Some(log_file_writer) = &log_file_writer {
        let temp_dir = workspace_directory(context.workspace_root_dir(), context.execution_id(), "logs");
        if let Ok(temp_dir) = temp_dir {
            log_file_writer.enable(&temp_dir);
        }
    }
}

fn disable_log_file_writer(log_file_writer: &Option<LogFileWriter>) {
    if let Some(log_file_writer) = log_file_writer {
        log_file_writer.disable();
    }
}
