use std::borrow::Borrow;
use std::fs;
use std::fs::{create_dir_all, File};
use std::io::{Error, ErrorKind, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use walkdir::WalkDir;

pub fn copy_files(from: &Path, to: &Path, exclude_j2_files: bool) -> Result<(), Error> {
    let files = WalkDir::new(from)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok());

    let files = match exclude_j2_files {
        true => files
            .filter(|e| {
                // return only non *.j2.* files
                e.file_name()
                    .to_str()
                    .map(|s| !s.contains(".j2."))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>(),
        false => files.collect::<Vec<_>>(),
    };

    let _ = fs::create_dir_all(to)?;
    let from_str = from.to_str().unwrap();

    for file in files {
        let path_str = file.path().to_str().unwrap();
        let dest = format!(
            "{}{}",
            to.to_str().unwrap(),
            path_str.replace(from_str, "").as_str()
        );

        if file.metadata().unwrap().is_dir() {
            let _ = fs::create_dir_all(&dest)?;
        }

        let _ = fs::copy(file.path(), dest);
    }

    Ok(())
}

pub fn workspace_directory<S, P>(execution_id: S, dir_name: P) -> String
where
    S: AsRef<Path>,
    P: AsRef<Path>,
{
    let dir = format!(
        ".qovery-workspace/{}/{}-{}",
        execution_id.as_ref().to_str().unwrap(),
        dir_name.as_ref().to_str().unwrap(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_millis()
    );

    let _ = create_dir_all(&dir);

    dir
}
