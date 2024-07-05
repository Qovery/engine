use std::collections::HashSet;
use std::fs;
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{BufRead, BufReader, Error, ErrorKind, Write};
use std::path::{Path, PathBuf};

use crate::cmd::structs::SecretItem;
use crate::errors::CommandError;
use base64::engine::general_purpose;
use base64::Engine;
use flate2::write::GzEncoder;
use flate2::Compression;
use itertools::Itertools;
use serde::__private::from_utf8_lossy;
use std::ffi::OsStr;
use walkdir::WalkDir;

pub fn delete_file_if_exists(file: &Path) -> Result<(), Error> {
    if !file.exists() {
        return Ok(());
    }

    fs::remove_file(file)
}

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

    create_dir_all(to)?;
    let from_str = from.to_str().unwrap();

    for file in files {
        let path_str = file.path().to_str().unwrap();
        let dest = format!("{}{}", to.to_str().unwrap(), path_str.replace(from_str, "").as_str());

        if file.metadata().unwrap().is_dir() {
            create_dir_all(&dest)?;
        }

        let _ = fs::copy(file.path(), dest);
    }

    Ok(())
}

pub fn root_workspace_directory<X, S>(working_root_dir: X, execution_id: S) -> Result<PathBuf, Error>
where
    X: AsRef<Path>,
    S: AsRef<Path>,
{
    workspace_directory(working_root_dir, execution_id, ".")
}

pub fn workspace_directory<X, S, P>(working_root_dir: X, execution_id: S, dir_name: P) -> Result<PathBuf, Error>
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

    Ok(dir)
}

fn archive_workspace_directory(working_root_dir: &str, execution_id: &str) -> Result<PathBuf, Error> {
    let workspace_dir = root_workspace_directory(working_root_dir, execution_id)?;
    let tgz_file_path = PathBuf::from(format!("{working_root_dir}/.qovery-workspace/{execution_id}.tgz").as_str());
    let tgz_file = File::create(&tgz_file_path)?;

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
            .strip_prefix(&workspace_dir)
            .map_err(|err| Error::new(ErrorKind::InvalidInput, err))?;

        tar.append_path_with_name(entry, relative_path)?;
    }

    Ok(tgz_file_path)
}

pub fn cleanup_workspace_directory(working_root_dir: &str, execution_id: &str) -> Result<(), Error> {
    return match root_workspace_directory(working_root_dir, execution_id) {
        Ok(workspace_dir) => {
            let workspace_dir = workspace_dir.to_string_lossy();
            match fs::remove_dir_all(match workspace_dir.strip_suffix("/.") {
                Some(striped_workspace_dir) => striped_workspace_dir, // Removing extra dir name allowing to delete directory properly ("/dir/." => "dir")
                None => workspace_dir.as_ref(),
            }) {
                Ok(_) => Ok(()),
                Err(err) => {
                    error!("error trying to remove workspace directory '{}', error: {}", workspace_dir, err);
                    Err(err)
                }
            }
        }
        Err(err) => {
            error!("error trying to get workspace directory from working_root_dir: '{working_root_dir}' execution_id: {execution_id}, error: {err}");
            Err(err)
        }
    };
}

pub fn create_workspace_archive(working_root_dir: &str, execution_id: &str) -> Result<PathBuf, Error> {
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

pub fn create_yaml_backup_file<P>(
    working_root_dir: P,
    chart_name: String,
    resource_name: Option<String>,
    content: String,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let dir = working_root_dir.as_ref().join("backups");

    if let Err(e) = create_dir_all(&dir) {
        return Err(CommandError::new(
            "Unable to create root dir path.".to_string(),
            Some(e.to_string()),
            None,
        ));
    }

    let root_path = dir
        .to_str()
        .map(|e| e.to_string())
        .ok_or_else(|| CommandError::new_from_safe_message("Unable to get backups root dir path.".to_string()));

    let string_path = match resource_name.is_some() {
        true => format!(
            "{}/{}-{}-q-backup.yaml",
            root_path?,
            chart_name,
            resource_name.as_ref().unwrap()
        ),
        false => format!("{}/{}.yaml", root_path?, chart_name),
    };
    let str_path = string_path.as_str();
    let path = Path::new(str_path);

    let mut file = match File::create(path) {
        Err(e) => {
            return Err(CommandError::new(
                format!("Unable to create YAML backup file for chart {chart_name}."),
                Some(e.to_string()),
                None,
            ))
        }
        Ok(file) => file,
    };

    match file.write_all(content.as_bytes()) {
        Err(e) => Err(CommandError::new(
            format!("Unable to edit YAML backup file for chart {chart_name}."),
            Some(e.to_string()),
            None,
        )),
        Ok(_) => Ok(path.to_str().map(|e| e.to_string()).ok_or_else(|| {
            CommandError::new_from_safe_message(format!("Unable to get YAML backup file path for chart {chart_name}."))
        })?),
    }
}

pub fn remove_lines_starting_with(path: String, starters: Vec<&str>) -> Result<String, CommandError> {
    let file = OpenOptions::new().read(true).open(path.as_str()).map_err(|e| {
        CommandError::new(format!("Unable to open YAML backup file {path}."), Some(e.to_string()), None)
    })?;

    let mut content = BufReader::new(file.try_clone().unwrap())
        .lines()
        .map(|line| line.unwrap())
        .collect::<Vec<String>>();

    for starter in starters {
        content = content
            .into_iter()
            .filter(|line| !line.contains(starter))
            .collect::<Vec<String>>()
    }

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path.as_str())
        .map_err(|e| {
            CommandError::new(format!("Unable to edit YAML backup file {path}."), Some(e.to_string()), None)
        })?;

    match file.write_all(content.join("\n").as_bytes()) {
        Err(e) => Err(CommandError::new(
            format!("Unable to edit YAML backup file {path}."),
            Some(e.to_string()),
            None,
        )),
        Ok(_) => Ok(path),
    }
}

pub fn truncate_file_from_word(path: String, truncate_from: &str) -> Result<String, CommandError> {
    let file = OpenOptions::new().read(true).open(path.as_str()).map_err(|e| {
        CommandError::new(format!("Unable to open YAML backup file {path}."), Some(e.to_string()), None)
    })?;

    let content_vec = BufReader::new(file.try_clone().unwrap())
        .lines()
        .map(|line| line.unwrap())
        .collect::<Vec<String>>();

    let truncate_from_index = match content_vec.iter().rposition(|line| line.contains(truncate_from)) {
        None => content_vec.len(),
        Some(index) => index,
    };

    let content = Vec::from(&content_vec[..truncate_from_index]).join("\n");

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path.as_str())
        .map_err(|e| {
            CommandError::new(format!("Unable to edit YAML backup file {path}."), Some(e.to_string()), None)
        })?;

    match file.write_all(content.as_bytes()) {
        Err(e) => Err(CommandError::new(
            format!("Unable to edit YAML backup file {path}."),
            Some(e.to_string()),
            None,
        )),
        Ok(_) => Ok(path),
    }
}

pub fn indent_file(path: String) -> Result<String, CommandError> {
    let file = OpenOptions::new().read(true).open(path.as_str()).map_err(|e| {
        CommandError::new(format!("Unable to open YAML backup file {path}."), Some(e.to_string()), None)
    })?;

    let file_content = BufReader::new(file.try_clone().unwrap())
        .lines()
        .map(|line| line.unwrap())
        .collect::<Vec<String>>();

    let content = file_content.iter().map(|line| line[2..].to_string()).join("\n");

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path.as_str())
        .map_err(|e| {
            CommandError::new(format!("Unable to edit YAML backup file {path}."), Some(e.to_string()), None)
        })?;

    match file.write_all(content.as_bytes()) {
        Err(e) => Err(CommandError::new(
            format!("Unable to edit YAML backup file {path}."),
            Some(e.to_string()),
            None,
        )),
        Ok(_) => Ok(path),
    }
}

pub fn list_yaml_backup_files<P>(working_root_dir: P) -> Result<Vec<String>, CommandError>
where
    P: AsRef<Path>,
{
    let files = WalkDir::new(working_root_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok());
    let mut backup_paths: Vec<String> = vec![];
    for file in files {
        if file
            .file_name()
            .to_str()
            .ok_or_else(|| {
                CommandError::new_from_safe_message(format!("Unable to get YAML backup file name {file:?}."))
            })?
            .to_string()
            .contains("-q-backup.yaml")
        {
            backup_paths.push(
                file.path()
                    .to_str()
                    .ok_or_else(|| {
                        CommandError::new_from_safe_message(format!("Unable to get YAML backup file name {file:?}."))
                    })?
                    .to_string(),
            )
        }
    }

    if backup_paths.is_empty() {
        return Err(CommandError::new_from_safe_message(
            "Unable to get YAML backup files".to_string(),
        ));
    }

    Ok(backup_paths)
}

pub fn create_yaml_file_from_secret<P>(working_root_dir: P, secret: SecretItem) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let message = format!("Unable to decode secret {}", secret.metadata.name);
    let secret_data = secret.data.values().next();
    let secret_content = match secret_data.is_some() {
        true => secret_data.unwrap().to_string(),
        false => return Err(CommandError::new_from_safe_message(message)),
    };

    let content = match general_purpose::STANDARD.decode(secret_content) {
        Ok(bytes) => from_utf8_lossy(&bytes[1..bytes.len() - 1]).to_string(),
        Err(e) => return Err(CommandError::new(message, Some(e.to_string()), None)),
    };
    match create_yaml_backup_file(working_root_dir.as_ref(), secret.metadata.name.clone(), None, content) {
        Ok(path) => Ok(path),
        Err(e) => Err(CommandError::new(
            format!("Unable to create backup file from secret {}", secret.metadata.name),
            Some(e.to_string()),
            None,
        )),
    }
}

#[cfg(test)]
mod tests {
    extern crate tempdir;

    use super::*;
    use flate2::read::GzDecoder;
    use std::collections::HashSet;
    use std::fs::File;
    use std::io::BufReader;
    use tempdir::TempDir;

    #[test]
    fn test_archive_workspace_directory() {
        // setup:
        let execution_id: &str = "123";
        let tmp_dir = TempDir::new("workspace_directory").expect("error creating temporary dir");
        let root_dir = format!("{}/.qovery-workspace/{}", tmp_dir.path().to_str().unwrap(), execution_id);
        let root_dir_path = Path::new(root_dir.as_str());

        let directories_to_create = vec![
            root_dir.to_string(),
            format!("{root_dir}/.terraform"),
            format!("{root_dir}/.terraform/dir-1"),
            format!("{root_dir}/dir-1"),
            format!("{root_dir}/dir-1/.terraform"),
            format!("{root_dir}/dir-1/.terraform/dir-1"),
            format!("{root_dir}/dir-2"),
            format!("{root_dir}/dir-2/.terraform"),
            format!("{root_dir}/dir-2/dir-1/.terraform"),
            format!("{root_dir}/dir-2/dir-1/.terraform/dir-1"),
            format!("{root_dir}/dir-2/.terraform/dir-1"),
        ];
        directories_to_create
            .iter()
            .for_each(|d| create_dir_all(d).expect("error creating directory"));

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
        .into_iter()
        .map(|(p, c)| {
            let mut file = File::create(root_dir_path.join(p)).expect("error creating file");
            file.write_all(c.as_bytes()).expect("error writing into file");

            file
        })
        .collect::<Vec<File>>();

        // execute:
        let result =
            archive_workspace_directory(tmp_dir.path().to_str().expect("error getting file path string"), execution_id);

        // verify:
        assert!(result.is_ok());

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
            assert!(files_in_tar.contains(e));
        }
        for e in files_in_tar.iter() {
            assert!(expected_files_in_tar.contains(e));
        }

        // clean:
        tmp_files.iter().for_each(drop);
        tmp_dir.close().expect("error closing temporary directory");
    }

    #[test]
    fn test_backup_cleaning() {
        let content = r#"
          apiVersion: cert-manager.io/v1
          kind: Certificate
          metadata:
            annotations:
              meta.helm.sh/release-name: cert-manager-configs
              meta.helm.sh/release-namespace: cert-manager
            creationTimestamp: "2021-11-04T10:26:27Z"
            generation: 2
            labels:
              app.kubernetes.io/managed-by: Helm
            name: qovery
            namespace: qovery
            resourceVersion: "28347460"
            uid: 509aad5f-db2d-44c3-b03b-beaf144118e2
          spec:
            dnsNames:
            - 'qovery'
            issuerRef:
              kind: ClusterIssuer
              name: qovery
            secretName: qovery
          status:
            conditions:
            - lastTransitionTime: "2021-11-30T15:33:03Z"
              message: Certificate is up to date and has not expired
              reason: Ready
              status: "True"
              type: Ready
            notAfter: "2022-04-29T13:34:51Z"
            notBefore: "2022-01-29T13:34:52Z"
            renewalTime: "2022-03-30T13:34:51Z"
            revision: 3
        "#;

        let tmp_dir = TempDir::new("workspace_directory").expect("error creating temporary dir");
        let mut file_path = create_yaml_backup_file(
            tmp_dir.path().to_str().unwrap(),
            "test".to_string(),
            Some("test".to_string()),
            content.to_string(),
        )
        .expect("No such file");
        file_path = remove_lines_starting_with(file_path, vec!["resourceVersion", "uid"]).unwrap();

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(file_path)
            .expect("file doesn't exist");

        let result = BufReader::new(file.try_clone().unwrap())
            .lines()
            .map(|line| line.unwrap())
            .collect::<Vec<String>>()
            .join("\n");

        let new_content = r#"
          apiVersion: cert-manager.io/v1
          kind: Certificate
          metadata:
            annotations:
              meta.helm.sh/release-name: cert-manager-configs
              meta.helm.sh/release-namespace: cert-manager
            creationTimestamp: "2021-11-04T10:26:27Z"
            generation: 2
            labels:
              app.kubernetes.io/managed-by: Helm
            name: qovery
            namespace: qovery
          spec:
            dnsNames:
            - 'qovery'
            issuerRef:
              kind: ClusterIssuer
              name: qovery
            secretName: qovery
          status:
            conditions:
            - lastTransitionTime: "2021-11-30T15:33:03Z"
              message: Certificate is up to date and has not expired
              reason: Ready
              status: "True"
              type: Ready
            notAfter: "2022-04-29T13:34:51Z"
            notBefore: "2022-01-29T13:34:52Z"
            renewalTime: "2022-03-30T13:34:51Z"
            revision: 3
        "#
        .to_string();

        assert_eq!(result, new_content);
        drop(file);
        tmp_dir.close().expect("error closing temporary directory");
    }
}
