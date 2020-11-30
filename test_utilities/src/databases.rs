use crate::utilities::generate_id;
use qovery_engine::models::{Environment, Kind, Context, Action, Database, DatabaseKind};
use crate::aws::ORGANIZATION_ID;

pub fn only_dev_postgresql_database(context: &Context) -> Environment {
    let database_port = 5432;
    let database_username = "superuser".to_string();
    let database_password = generate_id();
    let database_name = "my-psql".to_string();
    let fqdn_id = "my-postgresql-".to_string() + generate_id().as_str();
    let fqdn = fqdn_id.clone() + ".oom.sh";

    Environment {
        execution_id: context.execution_id().to_string(),
        id: generate_id(),
        kind: Kind::Development,
        owner_id: generate_id(),
        project_id: generate_id(),
        organization_id: ORGANIZATION_ID.to_string(),
        action: Action::Create,
        applications: vec![],
        routers: vec![],
        databases: vec![
            Database {
                kind: DatabaseKind::Postgresql,
                action: Action::Create,
                id: generate_id(),
                name: database_name.clone(),
                version: "11.8.0".to_string(),
                fqdn_id: fqdn_id.clone(),
                fqdn: fqdn.clone(),
                port: database_port.clone(),
                username: database_username.clone(),
                password: database_password.clone(),
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 512,
                disk_size_in_gib: 10,
                database_instance_type: "db.t2.micro".to_string(),
                database_disk_type: "gp2".to_string(),
            },
        ],
        external_services: vec![],
        clone_from_environment_id: None,
    }
}