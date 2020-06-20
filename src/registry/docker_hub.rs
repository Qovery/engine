use crate::registry::{PushError, PushImage, PushResult, Registry};

pub struct DockerHub {}

impl<'a> Registry<'a> for DockerHub {
    fn is_valid(&self) -> bool {
        true
    }

    fn push(&self, image: PushImage<'a>) -> Result<PushResult<'a>, PushError> {
        unimplemented!()
    }
}
