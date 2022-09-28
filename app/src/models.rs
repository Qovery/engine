#[derive(Clone, Copy, Debug)]
pub enum TaskSelector {
    Infrastructure(&'static str),
    Environment(&'static str),
}
