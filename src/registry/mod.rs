mod docker_hub;

pub trait Registry {
    fn is_valid(&self) -> bool;
}
