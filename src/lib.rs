#[cfg(feature = "pyo3")]
use pyo3::prelude::*;
#[cfg(feature = "pyo3")]
use pyo3_log;

mod error;
mod evaluator;
mod link;
mod source;

pub use error::{Error, ErrorKind};
pub use link::Link;

/// A Python module implemented in Rust.
#[cfg(feature = "pyo3")]
#[pymodule]
#[pyo3(name = "_core")]
fn unearth(_py: Python, m: &PyModule) -> PyResult<()> {
    pyo3_log::init();

    m.add_class::<Link>()?;
    Ok(())
}
