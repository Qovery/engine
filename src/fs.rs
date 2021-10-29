use std::collections::HashSet;
use std::fs;
use std::fs::{create_dir_all, File};
use std::io::{Error, ErrorKind};
use std::path::Path;

use flate2::write::GzEncoder;
use flate2::Compression;
use std::ffi::OsStr;
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

pub fn root_workspace_directory<X, S>(working_root_dir: X, execution_id: S) -> Result<String, std::io::Error>
where
    X: AsRef<Path>,
    S: AsRef<Path>,
{
    workspace_directory(working_root_dir, execution_id, ".")
}

pub fn workspace_directory<X, S, P>(working_root_dir: X, execution_id: S, dir_name: P) -> Result<String, std::io::Error>
where
    X: AsRef<Path>,
    S: AsRef<Path>,
    P: AsRef<Path>,
{
    let dir = working_root_dir
        .as_ref()
        .join(".qovery-workspace")
        .join(execution_id)
        .join(dir_name);

    create_dir_all(&dir)?;

    dir.to_str()
        .map(|e| e.to_string())
        .ok_or_else(|| Error::from(ErrorKind::NotFound))
}

fn archive_workspace_directory(working_root_dir: &str, execution_id: &str) -> Result<String, std::io::Error> {
    let workspace_dir = crate::fs::root_workspace_directory(working_root_dir, execution_id)?;
    let tgz_file_path = format!("{}/.qovery-workspace/{}.tgz", working_root_dir, execution_id);
    let tgz_file = File::create(tgz_file_path.as_str())?;

    let enc = GzEncoder::new(tgz_file, Compression::fast());
    let mut tar = tar::Builder::new(enc);
    let excluded_files: HashSet<&'static OsStr> = vec![OsStr::new(".terraform.lock.hcl"), OsStr::new(".terraform")]
        .into_iter()
        .collect();

    for entry in WalkDir::new(&workspace_dir)
        .into_iter()
        .filter_entry(|e| !excluded_files.contains(&e.file_name()))
    {
        let entry = match &entry {
            Ok(val) => val.path(),
            Err(err) => {
                error!("Cannot read file {:?}", err);
                continue;
            }
        };

        if !entry.is_file() {
            continue;
        }

        let relative_path = entry
            .strip_prefix(workspace_dir.as_str())
            .map_err(|err| Error::new(ErrorKind::InvalidInput, err))?;

        tar.append_path_with_name(entry, relative_path)?;
    }

    Ok(tgz_file_path)
}

pub fn cleanup_workspace_directory(working_root_dir: &str, execution_id: &str) -> Result<(), std::io::Error> {
    return match crate::fs::root_workspace_directory(working_root_dir, execution_id) {
        Ok(workspace_dir) => match std::fs::remove_dir_all(match workspace_dir.strip_suffix("/.") {
            Some(striped_workspace_dir) => striped_workspace_dir, // Removing extra dir name allowing to delete directory properly ("/dir/." => "dir")
            None => &(*workspace_dir.as_str()),
        }) {
            Ok(_) => Ok(()),
            Err(err) => {
                error!(
                    "{}",
                    format!(
                        "error trying to remove workspace directory '{}', error: {}",
                        workspace_dir.as_str(),
                        err
                    )
                );
                Err(err)
            }
        },
        Err(err) => {
            error!(
                "{}",
                format!(
                    "error trying to get workspace directory from working_root_dir: '{}' execution_id: {}, error: {}",
                    working_root_dir, execution_id, err
                )
            );
            Err(err)
        }
    };
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
            cleanup_workspace_directory(working_root_dir, execution_id)?;
            Ok(file)
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate tempdir;

    use super::*;
    use flate2::read::GzDecoder;
    use std::collections::HashSet;
    use std::fs::File;
    use std::io::prelude::*;
    use std::io::BufReader;
    use tempdir::TempDir;

    #[test]
    fn test_archive_workspace_directory() {
        // setup:
        let execution_id: &str = "123";
        let tmp_dir = TempDir::new("workspace_directory").expect("error creating temporary dir");
        let root_dir = format!(
            "{}/.qovery-workspace/{}",
            tmp_dir.path().to_str().unwrap(),
            execution_id
        );
        let root_dir_path = Path::new(root_dir.as_str());

        let directories_to_create = vec![
            root_dir.to_string(),
            format!("{}/.terraform", root_dir),
            format!("{}/.terraform/dir-1", root_dir),
            format!("{}/dir-1", root_dir),
            format!("{}/dir-1/.terraform", root_dir),
            format!("{}/dir-1/.terraform/dir-1", root_dir),
            format!("{}/dir-2", root_dir),
            format!("{}/dir-2/.terraform", root_dir),
            format!("{}/dir-2/dir-1/.terraform", root_dir),
            format!("{}/dir-2/dir-1/.terraform/dir-1", root_dir),
            format!("{}/dir-2/.terraform/dir-1", root_dir),
        ];
        directories_to_create
            .iter()
            .for_each(|d| fs::create_dir_all(d).expect("error creating directory"));

        let tmp_files = vec![
            (".terraform/file-1.txt", "content"),
            (".terraform/dir-1/file-1.txt", "content"),
            ("dir-1/.terraform/file-1.txt", "content"),
            ("dir-1/.terraform/dir-1/file-1.txt", "content"),
            ("dir-2/dir-1/.terraform/file-1.txt", "content"),
            ("dir-2/dir-1/.terraform/dir-1/file-1.txt", "content"),
            ("file-1.txt", "content"),
            (".terraform.lock.hcl", "content"),
            ("dir-1/.terraform.lock.hcl", "content"),
            ("dir-2/dir-1/.terraform.lock.hcl", "content"),
            ("dir-2/dir-1/file-2.txt", "content"),
        ]
        .iter()
        .map(|(p, c)| {
            let mut file = File::create(root_dir_path.join(p)).expect("error creating file");
            file.write_all(c.as_bytes()).expect("error writing into file");

            file
        })
        .collect::<Vec<File>>();

        // execute:
        let result = archive_workspace_directory(
            tmp_dir.path().to_str().expect("error getting file path string"),
            execution_id,
        );

        // verify:
        assert_eq!(true, result.is_ok());

        let expected_files_in_tar: HashSet<String> =
            vec![String::from("file-1.txt"), String::from("dir-2/dir-1/file-2.txt")]
                .into_iter()
                .collect();

        let archive = File::open(result.expect("error creating archive workspace directory"))
            .expect("error opening archive file");
        let archive = BufReader::new(archive);
        let archive = GzDecoder::new(archive);
        let mut archive = tar::Archive::new(archive);
        let mut files_in_tar = HashSet::new();

        for entry in archive.entries().expect("error getting archive entries") {
            let encoded_entry = entry.expect("error getting encoded entry");
            let encoded_entry_path = &encoded_entry.path().expect("error getting encoded entry path");
            files_in_tar.insert(
                encoded_entry_path
                    .to_str()
                    .expect("error getting encoded entry path string")
                    .to_string(),
            );
        }

        assert_eq!(expected_files_in_tar.len(), files_in_tar.len());
        for e in expected_files_in_tar.iter() {
            assert_eq!(true, files_in_tar.contains(e));
        }
        for e in files_in_tar.iter() {
            assert_eq!(true, expected_files_in_tar.contains(e));
        }

        // clean:
        tmp_files.into_iter().for_each(drop);
        tmp_dir.close().expect("error closing temporary directory");
    }
}
