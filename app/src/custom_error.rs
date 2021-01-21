use std::fmt;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EngineInitError {
    Regular(ErrorKind),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ErrorKind {
    LibsPathsMissing,
    LibsDirEmpty,
    BinVersion,
    MissingBin,
}

impl Into<&str> for ErrorKind {
    fn into(self) -> &'static str {
        match self {
            ErrorKind::LibsPathsMissing => {
                "Libs directory is empty, ensure you have really download, and override the global vars"
            }
            ErrorKind::LibsDirEmpty => "Libs directory is empty, ensure you have really download the libs resources",
            ErrorKind::BinVersion => "Binary version used is not that you should use",
            ErrorKind::MissingBin => "Binary is not installed, please install it!",
        }
    }
}

impl fmt::Display for EngineInitError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            EngineInitError::Regular(ref err) => write!(f, "A regular error occurred {:?}", err),
        }
    }
}
