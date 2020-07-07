pub fn subject<'a>(mode: &'a Mode, subject: &'a str) -> String {
    match mode {
        Mode::Local => format!("engine.local.{}", subject),
        Mode::Cloud(cloud_provider, region, customer) => format!(
            "engine.cloud.{}.{}.{}.{}",
            customer, cloud_provider, region, subject
        ),
    }
}

pub type CloudProvider<'a> = &'a str;
pub type Region<'a> = &'a str;
pub type Customer<'a> = &'a str;

pub enum Mode<'a> {
    Local,
    Cloud(Customer<'a>, CloudProvider<'a>, Region<'a>),
}
