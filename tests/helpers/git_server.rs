use testcontainers::core::{ExecCommand, IntoContainerPort, WaitFor};
use testcontainers::runners::SyncRunner;
use testcontainers::{Container, GenericImage};

pub fn init_git_server_testcontainer(repo_id: String) -> Container<GenericImage> {
    // see https://github.com/rockstorm101/gitweb-docker
    let container = GenericImage::new("rockstorm/gitweb", "2.43")
        .with_exposed_port(80.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Configuration complete; ready for start up"))
        .start()
        .expect("GitWeb Started");
    container
        .exec(ExecCommand::new(vec![
            "sh",
            "-c",
            "echo -ne \"[safe]\n\tdirectory = *\" > /var/run/fcgiwrap/.gitconfig",
        ]))
        .expect("Created git config");
    container
        .exec(ExecCommand::new(vec![
            "sh",
            "-c",
            format!(
                "git clone -b basic-app-deploy https://github.com/Qovery/engine-testing.git /srv/git/{repo_id}.git"
            )
            .as_str(),
        ]))
        .expect("Cloned repo");
    container
}
