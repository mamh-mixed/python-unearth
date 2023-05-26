use lazy_static::lazy_static;
#[cfg(feature = "pyo3")]
use pyo3::{
    basic::CompareOp,
    exceptions::{PyNotImplementedError, PyValueError},
    prelude::*,
    types::{PyDict, PyType},
};
use regex::Regex;
use serde::Deserialize;
use std::path::Path;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};
use std::{collections::HashMap, fmt, path::PathBuf, str::FromStr};
use url::Url;

use crate::error::{Error, ErrorKind};

lazy_static! {
    static ref VCS_SCHEMES: [&'static str; 4] = ["git", "hg", "svn", "bzr"];
    static ref SUPPORTED_HASHES: [&'static str; 6] =
        ["sha1", "sha224", "sha384", "sha256", "sha512", "md5"];
    static ref SSH_GIT_URL: Regex = Regex::new(r"(^.+?://(?:.+?@)?.+?)(:)(.+$)").unwrap();
}

#[cfg_attr(feature = "pyo3", derive(FromPyObject))]
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum DistMetadata {
    Enabled(bool),
    Hashes(HashMap<String, String>),
}

#[cfg(feature = "pyo3")]
impl IntoPy<PyObject> for DistMetadata {
    fn into_py(self, py: Python) -> PyObject {
        match self {
            DistMetadata::Enabled(enabled) => enabled.into_py(py),
            DistMetadata::Hashes(hashes) => {
                let dict = PyDict::new(py);
                for (k, v) in hashes {
                    dict.set_item(k, v).unwrap();
                }
                dict.into_py(py)
            }
        }
    }
}

#[cfg_attr(feature = "pyo3", pyclass)]
#[derive(Clone, Debug)]
pub struct Link {
    url: String,
    pub normalized: String,
    pub parsed: Url,
    pub vcs: Option<String>,
    pub comes_from: Option<String>,
    pub yank_reason: Option<String>,
    pub requires_python: Option<String>,
    pub hashes_map: Option<HashMap<String, String>>,
    pub dist_metadata: Option<DistMetadata>,
}

/// Add ssh:// to git+ URLs if they don't already have it
/// This is what pip does.
fn add_ssh_scheme_to_git_uri(uri: &str) -> String {
    if uri.contains("://") {
        uri.to_string()
    } else {
        let cloned = format!("ssh://{}", uri);
        SSH_GIT_URL.replace(&cloned, "$1/$3").into_owned()
    }
}

impl Link {
    pub fn new(
        url: String,
        comes_from: Option<String>,
        yank_reason: Option<String>,
        requires_python: Option<String>,
        hashes: Option<HashMap<String, String>>,
        dist_metadata: Option<DistMetadata>,
    ) -> Result<Self, Error> {
        let cloned = url.clone();
        let (normalized, vcs) = {
            if VCS_SCHEMES
                .iter()
                .any(|&s| cloned.starts_with(format!("{}+", s).as_str()))
            {
                match cloned.split_once('+') {
                    Some((vcs, rest)) => {
                        let cleaned = add_ssh_scheme_to_git_uri(rest);
                        (cleaned, Some(vcs.to_string()))
                    }
                    None => (cloned, None),
                }
            } else {
                (cloned, None)
            }
        };
        let parsed = Url::parse(normalized.as_str())
            .map_err(|_| Error::new(ErrorKind::UrlError, format!("Invalid URL: {}", normalized)))?;
        Ok(Self {
            url,
            normalized,
            parsed,
            vcs,
            comes_from,
            yank_reason,
            requires_python,
            hashes_map: hashes,
            dist_metadata,
        })
    }

    pub fn is_file(&self) -> bool {
        self.parsed.scheme() == "file"
    }

    pub fn file_path(&self) -> Result<PathBuf, Error> {
        if self.is_file() {
            // file:// url to path
            self.parsed.to_file_path().map_err(|_| {
                Error::new(
                    ErrorKind::UrlError,
                    format!("Invalid file URL: {}", self.normalized),
                )
            })
        } else {
            Err(Error::new(
                ErrorKind::UrlError,
                format!("Not a file URL: {}", self.normalized),
            ))
        }
    }
    pub fn filename(&self) -> String {
        let path = self.parsed.path();
        let decoded_path = percent_encoding::percent_decode(path.as_bytes())
            .decode_utf8_lossy()
            .into_owned();
        let path = Path::new(&decoded_path);
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }

    pub fn url_without_fragment(&self) -> String {
        let mut cloned = self.parsed.clone();
        cloned.set_fragment(None);
        cloned.to_string()
    }

    pub fn is_wheel(&self) -> bool {
        self.filename().ends_with(".whl")
    }

    pub fn hashes(&self) -> Option<HashMap<String, String>> {
        if let Some(hashes) = &self.hashes_map {
            Some(hashes.clone())
        } else {
            let fragments = self.parsed.fragment()?;
            let query = url::form_urlencoded::parse(fragments.as_bytes());
            let hashes = query
                .into_iter()
                .filter(|(key, _)| SUPPORTED_HASHES.contains(&key.as_ref()))
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<HashMap<_, _>>();
            if hashes.is_empty() {
                None
            } else {
                Some(hashes)
            }
        }
    }

    pub fn egg(&self) -> Option<String> {
        let fragments = self.parsed.fragment()?;
        let query = url::form_urlencoded::parse(fragments.as_bytes());
        let egg = query
            .into_iter()
            .find(|(key, _)| key == "egg")
            .map(|(_, value)| value.to_string());
        egg
    }
}

impl FromStr for Link {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.to_string(), None, None, None, None, None)
    }
}

impl Hash for Link {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.normalized.hash(state);
        self.requires_python.hash(state);
        self.yank_reason.hash(state);
    }
}

impl PartialEq for Link {
    fn eq(&self, other: &Self) -> bool {
        self.normalized == other.normalized
            && self.requires_python == other.requires_python
            && self.yank_reason == other.yank_reason
    }
}

impl Eq for Link {}

impl fmt::Display for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.normalized)
    }
}

#[cfg(feature = "pyo3")]
#[pymethods]
impl Link {
    #[new]
    #[pyo3(signature = (url, comes_from = None, yank_reason = None, requires_python = None, hashes = None, dist_metadata = None))]
    fn py_new(
        url: String,
        comes_from: Option<String>,
        yank_reason: Option<String>,
        requires_python: Option<String>,
        hashes: Option<HashMap<String, String>>,
        dist_metadata: Option<DistMetadata>,
    ) -> PyResult<Self> {
        Ok(Self::new(
            url,
            comes_from,
            yank_reason,
            requires_python,
            hashes,
            dist_metadata,
        )?)
    }

    fn __repr__(&self) -> String {
        format!(
            "<Link {} (from {})>",
            self.redacted(),
            self.comes_from.clone().unwrap_or_default()
        )
    }

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
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }

    #[getter]
    fn url(&self) -> &str {
        self.url.as_str()
    }

    #[getter]
    fn normalized(&self) -> &str {
        self.normalized.as_str()
    }

    #[getter]
    fn vcs(&self) -> Option<&str> {
        self.vcs.as_deref()
    }

    #[getter]
    fn comes_from(&self) -> Option<&str> {
        self.comes_from.as_deref()
    }

    #[getter]
    fn yank_reason(&self) -> Option<&str> {
        self.yank_reason.as_deref()
    }

    #[getter]
    fn requires_python(&self) -> Option<&str> {
        self.requires_python.as_deref()
    }

    #[getter]
    fn dist_metadata(&self) -> Option<DistMetadata> {
        self.dist_metadata.clone()
    }

    #[getter]
    fn redacted(&self) -> String {
        if !self.parsed.has_authority() {
            self.normalized.clone()
        } else {
            let mut cloned = self.parsed.clone();
            cloned.set_username("***").unwrap();
            cloned.set_password(None).unwrap();
            cloned.to_string()
        }
    }

    #[getter(url_without_fragment)]
    fn py_url_without_fragment(&self) -> String {
        self.url_without_fragment()
    }

    #[getter(is_file)]
    fn py_is_file(&self) -> bool {
        self.is_file()
    }

    #[getter(file_path)]
    fn py_file_path(&self) -> PyResult<String> {
        Ok(self.file_path()?.to_string_lossy().to_string())
    }

    #[classmethod]
    fn from_path(_cls: &PyType, path: String) -> PyResult<Self> {
        let url = Url::from_file_path(path).map_err(|_| {
            PyValueError::new_err("Invalid file path, must be absolute and contain no NUL bytes")
        })?;
        Ok(Self {
            url: url.to_string(),
            normalized: url.to_string(),
            parsed: url,
            vcs: None,
            comes_from: None,
            yank_reason: None,
            requires_python: None,
            hashes_map: None,
            dist_metadata: None,
        })
    }

    #[getter]
    fn is_vcs(&self) -> bool {
        self.vcs.is_some()
    }

    #[getter(is_wheel)]
    fn py_is_wheel(&self) -> bool {
        self.is_wheel()
    }

    #[getter]
    fn is_yanked(&self) -> bool {
        self.yank_reason.is_some()
    }

    #[getter(filename)]
    fn py_filename(&self) -> String {
        self.filename()
    }

    #[getter]
    fn subdirectory(&self) -> Option<String> {
        let fragments = self.parsed.fragment()?;
        // parse the fragments as query dict
        let query = url::form_urlencoded::parse(fragments.as_bytes());
        // get the subdirectory key
        query
            .into_iter()
            .find(|(key, _)| key == "subdirectory")
            .map(|(_, value)| value.to_string())
    }

    #[getter(hashes)]
    fn py_hashes(&self) -> Option<HashMap<String, String>> {
        self.hashes()
    }

    #[getter]
    fn dist_metadata_link(&self) -> Option<Self> {
        match self.dist_metadata {
            Some(DistMetadata::Enabled(true)) | Some(DistMetadata::Hashes(_)) => Some(
                Self::py_new(
                    format!("{}.metadata", self.url_without_fragment()),
                    self.comes_from.clone(),
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap(),
            ),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let link = Link::new(
            "https://example.com/".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(link.url, "https://example.com/");
        assert_eq!(link.normalized, "https://example.com/");
        assert_eq!(link.vcs, None);
        assert_eq!(link.comes_from, None);
        assert_eq!(link.yank_reason, None);
        assert_eq!(link.requires_python, None);
        assert_eq!(link.hashes_map, None);
    }

    #[test]
    fn test_is_file() {
        let link = Link::new(
            "file:///path/to/file".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(link.is_file());
    }

    #[test]
    fn test_file_path() {
        let link = Link::new(
            "file:///path/to/file".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            link.file_path().unwrap(),
            PathBuf::from_str("/path/to/file").unwrap()
        );
    }
}
