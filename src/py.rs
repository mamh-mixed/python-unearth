use std::fmt;

#[cfg(feature = "pyo3")]
use pyo3::{exceptions::PyNotImplementedError, prelude::*, pyclass::CompareOp, types::PyIterator};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, FromPyObject)]
pub struct PythonVersion(u16, u16);

#[cfg(feature = "pyo3")]
impl IntoPy<PyObject> for PythonVersion {
    fn into_py(self, py: Python) -> PyObject {
        (self.0, self.1).into_py(py)
    }
}

impl PythonVersion {
    pub fn short_version(&self) -> String {
        format!("{}{}", self.0, self.1)
    }
}
#[cfg_attr(feature = "pyo3", pyclass(get_all))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag {
    pub interpreter: String,
    pub abi: String,
    pub platform: String,
}

impl Tag {
    fn from_pkg_obj(obj: &PyAny) -> PyResult<Self> {
        let interpreter = obj.getattr("interpreter")?.extract()?;
        let abi = obj.getattr("abi")?.extract()?;
        let platform = obj.getattr("platform")?.extract()?;

        Ok(Self {
            interpreter,
            abi,
            platform,
        })
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}-{}", self.interpreter, self.abi, self.platform)
    }
}

#[cfg(feature = "pyo3")]
#[pymethods]
impl Tag {
    fn __richcmp__(&self, other: &Self, op: CompareOp) -> PyResult<bool> {
        match op {
            CompareOp::Eq => Ok(self == other),
            CompareOp::Ne => Ok(self != other),
            _ => Err(PyNotImplementedError::new_err(
                "Only equality comparisons are supported",
            )),
        }
    }

    fn __hash__(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("Tag<{}>", self.to_string())
    }
}

#[cfg_attr(feature = "pyo3", pyclass(get_all))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TargetPython {
    pub supported_tags: Vec<Tag>,
}

impl TargetPython {
    pub fn new(supported_tags: Vec<Tag>) -> Self {
        Self { supported_tags }
    }
}

#[cfg(feature = "pyo3")]
#[pymethods]
impl TargetPython {
    #[new]
    #[pyo3(signature = (py_ver = None, abis = None, implementation = None, platforms = None))]
    fn py_new(
        py_ver: Option<PythonVersion>,
        abis: Option<Vec<String>>,
        implementation: Option<String>,
        platforms: Option<Vec<String>>,
    ) -> PyResult<Self> {
        let tags = py_impl::get_supported_tags(py_ver, abis, implementation, platforms)?;

        Ok(Self::new(tags))
    }
}

#[cfg(feature = "pyo3")]
mod py_impl {
    use std::collections::HashSet;

    use super::*;

    pub fn get_supported_tags(
        py_ver: Option<PythonVersion>,
        abis: Option<Vec<String>>,
        implementation: Option<String>,
        platforms: Option<Vec<String>>,
    ) -> PyResult<Vec<Tag>> {
        let interpreter = get_custom_interpreter(implementation.as_deref(), py_ver.as_ref())?;

        let platforms = expand_allowed_platforms(platforms)?;

        Python::with_gil(|py| {
            let packaging_tags = py.import("packaging.tags")?;
            let interpreter_name: String = packaging_tags
                .getattr("interpreter_name")?
                .call0()?
                .extract()?;
            let is_cpython = implementation.unwrap_or(interpreter_name) == "cp";
            let mut tags: Vec<Tag> = vec![];

            if is_cpython {
                tags.extend(
                    packaging_tags
                        .getattr("cpython_tags")?
                        .call1((py_ver.clone(), abis.clone(), platforms.clone()))?
                        .extract::<&PyIterator>()?
                        .filter_map(|e| e.ok().and_then(|e| Tag::from_pkg_obj(e).ok())),
                );
            } else {
                tags.extend(
                    packaging_tags
                        .getattr("generic_tags")?
                        .call1((interpreter.clone(), abis.clone(), platforms.clone()))?
                        .extract::<&PyIterator>()?
                        .filter_map(|e| e.ok().and_then(|e| Tag::from_pkg_obj(e).ok())),
                );
            }
            tags.extend(
                packaging_tags
                    .getattr("compatible_tags")?
                    .call1((py_ver, interpreter, platforms))?
                    .extract::<&PyIterator>()?
                    .filter_map(|e| e.ok().and_then(|e| Tag::from_pkg_obj(e).ok())),
            );
            Ok::<_, PyErr>(tags)
        })
    }

    fn get_custom_interpreter(
        implementation: Option<&str>,
        version: Option<&PythonVersion>,
    ) -> PyResult<String> {
        let (pkg_impl, pkg_version) = Python::with_gil(|py| {
            let packaging_tags = py.import("packaging.tags")?;
            let packaging_impl: String = packaging_tags
                .getattr("interpreter_name")?
                .call0()?
                .extract()?;
            let packaging_version: String = packaging_tags
                .getattr("interpreter_version")?
                .call0()?
                .extract()?;
            Ok::<_, PyErr>((packaging_impl, packaging_version))
        })?;
        Ok(format!(
            "{}{}",
            implementation.unwrap_or(pkg_impl.as_str()),
            version.map(|v| v.short_version()).unwrap_or(pkg_version)
        ))
    }

    fn expand_allowed_platforms(platforms: Option<Vec<String>>) -> PyResult<Option<Vec<String>>> {
        let result = platforms.map(|values| {
            let mut seen = HashSet::new();
            let mut result = Vec::new();

            for p in values {
                if seen.contains(&p) {
                    continue;
                }
                let custom_platforms = get_custom_platforms(&p).unwrap_or_default();
                let additions = custom_platforms
                    .into_iter()
                    .filter(|c| !seen.contains(c))
                    .collect::<Vec<_>>();
                seen.extend(additions.iter().cloned());
                result.extend(additions);
            }
            result
        });
        Ok(result)
    }

    fn get_custom_platforms(arch: &str) -> PyResult<Vec<String>> {
        let arch_prefix = arch.split('_').next().unwrap();
        let arches = if arch.starts_with("macosx") {
            mac_platforms(arch)?
        } else if arch_prefix == "manylinux2014" || arch_prefix == "manylinux2010" {
            custom_manylinux_platforms(arch)
        } else {
            vec![arch.to_string()]
        };
        Ok(arches)
    }

    fn mac_platforms(arch: &str) -> PyResult<Vec<String>> {
        let osx_arch_pat = regex::Regex::new(r#"(.+)_(\d+)_(\d+)_(.+)"#).unwrap();
        if let Some(captures) = osx_arch_pat.captures(arch) {
            let name = captures.get(1).unwrap().as_str();
            let major = captures.get(2).unwrap().as_str().parse::<u32>().unwrap();
            let minor = captures.get(3).unwrap().as_str().parse::<u32>().unwrap();
            let actual_arch = captures.get(4).unwrap().as_str();
            let mac_version = (major, minor);
            let arches = Python::with_gil(|py| {
                let packaging_tags = py.import("packaging.tags")?;
                let mac_platforms: &PyIterator = packaging_tags
                    .getattr("mac_platforms")?
                    .call1((mac_version, actual_arch))?
                    .extract()?;
                let v: Vec<String> = mac_platforms
                    .filter_map(|p| p.ok()?.extract::<String>().ok())
                    .collect();
                Ok::<_, PyErr>(v)
            })?;
            Ok(arches
                .into_iter()
                .map(|arch| format!("{}_{}", name, arch))
                .collect::<Vec<_>>())
        } else {
            Ok(vec![arch.to_string()])
        }
    }

    fn custom_manylinux_platforms(arch: &str) -> Vec<String> {
        let mut arches = vec![arch.to_string()];
        let (arch_prefix, arch_suffix) = match arch.find('_') {
            Some(idx) => (&arch[..idx], &arch[idx + 1..]),
            None => (arch, ""),
        };
        if arch_prefix == "manylinux2014" {
            if arch_suffix == "i686" || arch_suffix == "x86_64" {
                arches.push(format!("manylinux2010_{}", arch_suffix));
                arches.push(format!("manylinux1_{}", arch_suffix));
            }
        } else if arch_prefix == "manylinux2010" {
            arches.push(format!("manylinux1_{}", arch_suffix));
        }
        arches
    }
}
