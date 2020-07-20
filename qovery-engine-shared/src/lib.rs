pub fn subject<'a>(mode: &'a Mode, subject: &'a str) -> String {
    match mode {
        Mode::Local => format!("engine.local.{}", subject),
        Mode::Cloud(organization, cloud_provider, region) => format!(
            "engine.cloud.{}.{}.{}.{}",
            organization, cloud_provider, region, subject
        ),
    }
}

pub type CloudProvider = String;
pub type Region = String;
pub type Organization = String;

pub enum Mode {
    Local,
    Cloud(Organization, CloudProvider, Region),
}
