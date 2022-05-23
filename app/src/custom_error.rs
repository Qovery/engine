use std::fmt;
use std::fmt::{Display, Formatter};

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

impl Display for ErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let msg = match self {
            ErrorKind::LibsPathsMissing => {
                "Libs directory is empty, ensure you have really download, and override the global vars"
            }
            ErrorKind::LibsDirEmpty => "Libs directory is empty, ensure you have really download the libs resources",
            ErrorKind::BinVersion => "Binary version used is not that you should use",
            ErrorKind::MissingBin => "Binary is not installed, please install it!",
        };
        f.write_str(msg)
    }
}

impl Display for EngineInitError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match *self {
            EngineInitError::Regular(ref err) => write!(f, "A regular error occurred {:?}", err),
        }
    }
}
