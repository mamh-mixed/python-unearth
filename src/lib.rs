use pyo3::prelude::*;

mod link;

use link::Link;

/// A Python module implemented in Rust.
#[pymodule]
#[pyo3(name = "_core")]
fn unearth(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Link>()?;
    Ok(())
}
