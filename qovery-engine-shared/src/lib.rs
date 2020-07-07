pub fn subject<'a>(mode: &'a Mode, subject: &'a str) -> String {
    match mode {
        Mode::Local => format!("engine.local.{}", subject),
        Mode::Cloud(cloud_provider, region, customer) => format!(
            "engine.cloud.{}.{}.{}.{}",
            customer, cloud_provider, region, subject
        ),
    }
}

pub type CloudProvider = String;
pub type Region = String;
pub type Customer = String;

pub enum Mode {
    Local,
    Cloud(Customer, CloudProvider, Region),
}
