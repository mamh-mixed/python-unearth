use std::{fmt, io};

#[cfg(feature = "pyo3")]
use pyo3::{
    exceptions::{PyOSError, PyValueError},
    prelude::*,
};

#[derive(Debug, PartialEq)]
pub enum ErrorKind {
    UrlError,
    UnpackError,
    HashError,
    IOError,
    CollectError,
    ValueError,
    LinkMismatchError,
}

#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

impl Error {
    pub fn new(kind: ErrorKind, message: String) -> Self {
        Self { kind, message }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self {
            kind: ErrorKind::IOError,
            message: error.to_string(),
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self {
            kind: ErrorKind::CollectError,
            message: error.to_string(),
        }
    }
}

#[cfg(feature = "pyo3")]
impl From<Error> for PyErr {
    fn from(error: Error) -> PyErr {
        match error.kind {
            ErrorKind::UnpackError => PyOSError::new_err(error.message),
            ErrorKind::CollectError => PyOSError::new_err(error.message),
            ErrorKind::IOError => PyOSError::new_err(error.message),
            _ => PyValueError::new_err(error.message),
        }
    }
}
