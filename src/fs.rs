use std::fs;
use std::fs::{create_dir_all, File};
use std::io::Error;
use std::path::Path;

use flate2::write::GzEncoder;
use flate2::Compression;
use walkdir::WalkDir;

pub fn copy_files(from: &Path, to: &Path, exclude_j2_files: bool) -> Result<(), Error> {
    let files = WalkDir::new(from).follow_links(true).into_iter().filter_map(|e| e.ok());

    let files = match exclude_j2_files {
        true => files
            .filter(|e| {
                // return only non *.j2.* files
                e.file_name().to_str().map(|s| !s.contains(".j2.")).unwrap_or(false)
            })
            .collect::<Vec<_>>(),
        false => files.collect::<Vec<_>>(),
    };

    let _ = fs::create_dir_all(to)?;
    let from_str = from.to_str().unwrap();

    for file in files {
        let path_str = file.path().to_str().unwrap();
        let dest = format!("{}{}", to.to_str().unwrap(), path_str.replace(from_str, "").as_str());

        if file.metadata().unwrap().is_dir() {
            let _ = fs::create_dir_all(&dest)?;
        }

        let _ = fs::copy(file.path(), dest);
    }

    Ok(())
}

pub fn root_workspace_directory<X, S>(working_root_dir: X, execution_id: S) -> String
where
    X: AsRef<Path>,
    S: AsRef<Path>,
{
    workspace_directory(working_root_dir, execution_id, ".")
}

pub fn workspace_directory<X, S, P>(working_root_dir: X, execution_id: S, dir_name: P) -> String
where
    X: AsRef<Path>,
    S: AsRef<Path>,
    P: AsRef<Path>,
{
    let dir = format!(
        "{}/.qovery-workspace/{}/{}",
        working_root_dir.as_ref().to_str().unwrap(),
        execution_id.as_ref().to_str().unwrap(),
        dir_name.as_ref().to_str().unwrap(),
    );

    let _ = create_dir_all(&dir);

    dir
}

fn archive_workspace_directory(working_root_dir: &str, execution_id: &str) -> Result<String, std::io::Error> {
    let workspace_dir = crate::fs::root_workspace_directory(working_root_dir, execution_id);

    let tar_gz_file_path = format!("{}/.qovery-workspace/{}.tar.gz", working_root_dir, execution_id);

    let tar_gz_file = File::create(tar_gz_file_path.as_str())?;

    let enc = GzEncoder::new(tar_gz_file, Compression::fast());
    let mut tar = tar::Builder::new(enc);
    tar.append_dir_all(execution_id, workspace_dir)?;

    Ok(tar_gz_file_path)
}

pub fn cleanup_workspace_directory(working_root_dir: &str, execution_id: &str) {
    let workspace_dir = crate::fs::root_workspace_directory(working_root_dir, execution_id);
    let _ = std::fs::remove_dir_all(workspace_dir);
}

pub fn create_workspace_archive(working_root_dir: &str, execution_id: &str) -> Result<String, std::io::Error> {
    info!("archive workspace directory in progress");

    match archive_workspace_directory(working_root_dir, execution_id) {
        Err(err) => {
            error!("archive workspace directory error: {:?}", err);
            Err(err)
        }
        Ok(file) => {
            info!("workspace directory is archived");
            cleanup_workspace_directory(working_root_dir, execution_id);
            Ok(file)
        }
    }
}
