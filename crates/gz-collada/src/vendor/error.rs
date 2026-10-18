// Modified for gzweb-rs; see `gz-collada:` markers.

use std::io;

macro_rules! format_err {
    ($msg:expr $(,)?) => {
        crate::vendor::error::invalid_data($msg)
    };
    ($($tt:tt)*) => {
        crate::vendor::error::invalid_data(format!($($tt)*))
    };
}

macro_rules! bail {
    ($($tt:tt)*) => {
        return Err(format_err!($($tt)*))
    };
}

#[cold]
pub(crate) fn invalid_data(e: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    let e = e.into();
    let kind = e
        .downcast_ref::<io::Error>()
        .map_or(io::ErrorKind::InvalidData, io::Error::kind);
    io::Error::new(kind, e)
}

// gz-collada: the OBJ/STL error locations are removed with those features.
