use curl::easy::Easy;
use curl::Error;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::models::{Context, Environment};

pub fn build_platform_local_docker(context: &Context) -> LocalDocker {
    LocalDocker::new(context.clone(), "oxqlm3r99vwcmvuj", "qovery-local-docker")
}

pub fn init() {
    env_logger::try_init();
    println!(
        "running from current directory: {}",
        std::env::current_dir().unwrap().to_str().unwrap()
    );
}

pub fn generate_id() -> String {
    // Should follow DNS naming convention https://tools.ietf.org/html/rfc1035
    let uuid;

    loop {
        let rand_string: String = thread_rng().sample_iter(Alphanumeric).take(15).collect();
        if rand_string.chars().next().unwrap().is_alphabetic() {
            uuid = rand_string.to_lowercase();
            break;
        }
    }
    uuid
}

pub fn check_all_connections(env: &Environment) -> Vec<bool> {
    let mut checking: Vec<bool> = Vec::with_capacity(env.routers.len());

    for router_to_test in &env.routers {
        let path_to_test = format!(
            "https://{}{}",
            &router_to_test.default_domain, &router_to_test.routes[0].path
        );

        checking.push(curl_path(path_to_test.as_str()));
    }
    return checking;
}

fn curl_path(path: &str) -> bool {
    let mut easy = Easy::new();
    easy.url(path).unwrap();
    let res = easy.perform();
    match res {
        Ok(out) => return true,

        Err(e) => {
            println!("TEST Error : while trying to call {}", e);
            return false;
        }
    }
}
