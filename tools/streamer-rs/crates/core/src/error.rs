use std::fmt;

#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub fn new(message: String) -> Self {
        Error(message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

impl From<Error> for String {
    fn from(e: Error) -> Self {
        e.0
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait Context<T> {
    fn context(self, message: &str) -> Result<T>;
    fn with_context(self, message: impl FnOnce() -> String) -> Result<T>;
}

impl<T> Context<T> for Option<T> {
    fn context(self, message: &str) -> Result<T> {
        self.ok_or_else(|| Error(message.into()))
    }

    fn with_context(self, message: impl FnOnce() -> String) -> Result<T> {
        self.ok_or_else(|| Error(message()))
    }
}

impl<T, E: fmt::Display> Context<T> for std::result::Result<T, E> {
    fn context(self, message: &str) -> Result<T> {
        self.map_err(|e| Error(format!("{message}: {e}")))
    }

    fn with_context(self, message: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|e| Error(format!("{}: {e}", message())))
    }
}

#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => {
        return Err($crate::error::Error::new(format!($($arg)*)))
    };
}
