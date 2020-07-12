use crate::cloud_provider::service::Service;

pub struct Environment {
    pub id: String,
    pub project_id: String,
    pub services: Vec<Box<dyn Service>>,
}

impl Environment {
    pub fn new(id: &str, project_id: &str) -> Self {
        Environment {
            id: id.to_string(),
            project_id: project_id.to_string(),
            services: vec![],
        }
    }

    pub fn namespace(&self) -> &str {
        self.project_id.as_str()
    }
}
