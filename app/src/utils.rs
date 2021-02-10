pub type CloudProvider = String;
pub type Region = String;
pub type Organization = String;

#[derive(Clone)]
pub enum Mode {
    Local,
    Cloud(Organization, CloudProvider, Region),
}
