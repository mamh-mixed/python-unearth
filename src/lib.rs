#[cfg(feature = "pyo3")]
use pyo3::prelude::*;
#[cfg(feature = "pyo3")]
use pyo3_log;

pub mod error;
pub mod evaluator;
mod hash;
pub mod link;
pub mod py;
pub mod session;
pub mod source;

pub use error::{Error, ErrorKind};
pub use link::Link;
pub use py::{Tag, TargetPython};

/// A Python module implemented in Rust.
#[cfg(feature = "pyo3")]
#[pymodule]
#[pyo3(name = "_core")]
fn unearth(_py: Python, m: &PyModule) -> PyResult<()> {
    pyo3_log::init();

    m.add_class::<Link>()?;
    m.add_class::<TargetPython>()?;
    m.add_class::<Tag>()?;
    Ok(())
}
