use crate::build_platform::registry::{PushError, PushImage, PushResult, Registry};

pub struct DockerHub<'a> {
    pub login: &'a str,
    pub password: &'a str,
}

impl<'a> Registry<'a> for DockerHub<'a> {
    fn is_valid(&self) -> bool {
        true
    }

    fn push(&self, image: PushImage<'a>) -> Result<PushResult<'a>, PushError> {
        unimplemented!()
    }
}
