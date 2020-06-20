mod docker_hub;

pub trait Registry<'a> {
    fn is_valid(&self) -> bool;
    fn push(&self, image: PushImage<'a>) -> Result<PushResult<'a>, PushError>;
}

pub struct PushImage<'a> {
    pub directory_path: &'a str,
    pub name: &'a str,
    pub tag: &'a str,
}

pub struct PushResult<'a> {
    pub image: PushImage<'a>,
}

pub enum PushError {
    ImageAlreadyExists,
}
