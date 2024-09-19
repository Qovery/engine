use git2::build::RepoBuilder;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use testcontainers::core::{AccessMode, IntoContainerPort, Mount, WaitFor};
use testcontainers::runners::SyncRunner;
use testcontainers::{Container, GenericImage, ImageExt};
use uuid::Uuid;

pub struct ContainerWithMountedFolders {
    pub container: Container<GenericImage>,
    pub mounted_folders: Vec<TempMountedDir>,
}

pub struct TempMountedDir {
    path: PathBuf,
}

impl TempMountedDir {
    // Generates a dir path with a custom suffix
    pub fn new_with_suffix(suffix: String) -> Self {
        let path = env::temp_dir().join(format!("{}_{}", Uuid::new_v4(), suffix));
        std::fs::create_dir(path.clone()).unwrap();
        TempMountedDir { path: path.clone() }
    }
    pub fn new_with_prefix(prefix: String) -> Self {
        let path = env::temp_dir().join(format!("{}_{}", prefix, Uuid::new_v4()));
        std::fs::create_dir(path.clone()).unwrap();
        TempMountedDir { path: path.clone() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempMountedDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub fn init_git_server_testcontainer() -> ContainerWithMountedFolders {
    let temp_dir_gitconfig = TempMountedDir::new_with_prefix("gitconfig".to_string());
    let git_server_config_pathbuf = temp_dir_gitconfig.path().join(".gitconfig");
    let mut file = File::create(git_server_config_pathbuf.as_path()).expect("File is created");
    file.write_all("[safe]\n\tdirectory = *".as_bytes())
        .expect("File is written");

    let repo_dir = TempMountedDir::new_with_suffix(".git".to_string());
    RepoBuilder::new()
        .bare(true)
        .branch("basic-app-deploy")
        .clone("https://github.com/Qovery/engine-testing.git", repo_dir.path())
        .expect("Repo is cloned");
    // see https://github.com/rockstorm101/gitweb-docker
    let container = GenericImage::new("rockstorm/gitweb", "2.43")
        .with_exposed_port(80.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Configuration complete; ready for start up"))
        .with_mount(Mount::bind_mount("/tmp", "/srv/git").with_access_mode(AccessMode::ReadOnly))
        .with_mount(
            Mount::bind_mount(
                git_server_config_pathbuf.as_os_str().to_str().expect("Path is valid"),
                "/var/run/fcgiwrap/.gitconfig",
            )
            .with_access_mode(AccessMode::ReadOnly),
        )
        .start()
        .expect("GitWeb Started");

    ContainerWithMountedFolders {
        container,
        mounted_folders: vec![repo_dir, temp_dir_gitconfig],
    }
}
