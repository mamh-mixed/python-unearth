use once_cell::sync::Lazy;
use pyo3::{
    exceptions::PyValueError,
    prelude::*,
    types::{PyDict, PyType},
};
use regex::Regex;
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::{collections::HashMap, path::Path};
use url::Url;

static VCS_SCHEMES: [&str; 4] = ["git", "hg", "svn", "bzr"];
static SUPPORTED_HASHES: [&str; 6] = ["sha1", "sha224", "sha384", "sha256", "sha512", "md5"];
static SSH_GIT_URL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(^.+?://(?:.+?@)?.+?)(:)(.+$)").unwrap());

#[derive(FromPyObject, Clone)]
pub enum DistMetadata {
    Enabled(bool),
    Hashes(HashMap<String, String>),
}

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

#[pyclass]
pub struct Link {
    #[pyo3(get)]
    url: String,
    #[pyo3(get)]
    normalized: String,
    parsed: Url,
    #[pyo3(get)]
    vcs: Option<String>,
    #[pyo3(get)]
    comes_from: Option<String>,
    #[pyo3(get)]
    yank_reason: Option<String>,
    #[pyo3(get)]
    requires_python: Option<String>,
    hashes: Option<HashMap<String, String>>,
    #[pyo3(get)]
    dist_metadata: Option<DistMetadata>,
}

/// Add ssh:// to git+ URLs if they don't already have it
/// This is what pip does.
fn add_ssh_scheme_to_git_uri(uri: &str) -> PyResult<String> {
    if uri.contains("://") {
        Ok(uri.to_string())
    } else {
        let cloned = format!("ssh://{}", uri);
        Ok(SSH_GIT_URL.replace(&cloned, "$1/$3").into_owned())
    }
}

#[pymethods]
impl Link {
    #[new]
    #[pyo3(signature = (url, comes_from = None, yank_reason = None, requires_python = None, hashes = None, dist_metadata = None))]
    fn new(
        url: String,
        comes_from: Option<String>,
        yank_reason: Option<String>,
        requires_python: Option<String>,
        hashes: Option<HashMap<String, String>>,
        dist_metadata: Option<DistMetadata>,
    ) -> PyResult<Self> {
        let cloned = url.clone();
        let (normalized, vcs) = {
            if VCS_SCHEMES
                .iter()
                .any(|&s| cloned.starts_with(format!("{}+", s).as_str()))
            {
                match cloned.split_once('+') {
                    Some((vcs, rest)) => {
                        let cleaned = add_ssh_scheme_to_git_uri(rest)?;
                        (cleaned, Some(vcs.to_string()))
                    }
                    None => (cloned, None),
                }
            } else {
                (cloned, None)
            }
        };
        let parsed = Url::parse(normalized.as_str())
            .map_err(|_| PyValueError::new_err(format!("Invalid URL: {}", normalized)))?;
        Ok(Self {
            url,
            normalized,
            parsed,
            vcs,
            comes_from,
            yank_reason,
            requires_python,
            hashes,
            dist_metadata,
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "<Link {} (from {})>",
            self.redacted(),
            self.comes_from.clone().unwrap_or_default()
        )
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

    #[getter]
    fn url_without_fragment(&self) -> String {
        let mut cloned = self.parsed.clone();
        cloned.set_fragment(None);
        cloned.to_string()
    }

    #[getter]
    fn is_file(&self) -> bool {
        self.parsed.scheme() == "file"
    }

    #[getter]
    fn file_path(&self) -> PyResult<String> {
        if self.is_file() {
            // file:// url to path
            match self.parsed.to_file_path() {
                Ok(path) => Ok(path.to_string_lossy().to_string()),
                Err(_) => Err(PyValueError::new_err("Invalid file URL")),
            }
        } else {
            Err(PyValueError::new_err("Not a file URL"))
        }
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
            hashes: None,
            dist_metadata: None,
        })
    }

    #[getter]
    fn is_vcs(&self) -> bool {
        self.vcs.is_some()
    }

    #[getter]
    fn is_yanked(&self) -> bool {
        self.yank_reason.is_some()
    }

    #[getter]
    fn filename(&self) -> PyResult<String> {
        let path = self.parsed.path();
        let decoded_path = percent_encoding::percent_decode(path.as_bytes())
            .decode_utf8_lossy()
            .into_owned();
        let path = Path::new(&decoded_path);
        Ok(path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned())
    }

    #[getter]
    fn is_wheel(&self) -> bool {
        self.filename().unwrap_or_default().ends_with(".whl")
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

    #[getter]
    fn hashes(&self) -> Option<HashMap<String, String>> {
        if let Some(hashes) = &self.hashes {
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

    #[getter]
    fn hash_options(&self) -> Option<HashMap<String, Vec<String>>> {
        self.hashes().map(|hashes| {
            hashes
                .into_iter()
                .map(|(key, value)| (key, vec![value]))
                .collect()
        })
    }

    #[getter]
    fn dist_metadata_link(&self) -> Option<Self> {
        match self.dist_metadata {
            Some(DistMetadata::Enabled(true)) | Some(DistMetadata::Hashes(_)) => Some(
                Self::new(
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

impl Serialize for Link {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Link", 5)?;
        state.serialize_field("url", &self.redacted())?;
        state.serialize_field("comes_from", &self.comes_from)?;
        state.serialize_field("yank_reason", &self.yank_reason)?;
        state.serialize_field("requires_python", &self.requires_python)?;
        state.serialize_field(
            "metadata",
            &self.dist_metadata_link().map(|link| link.normalized),
        )?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_without_fragment() {
        let link = Link::new(
            "https://example.com/#fragment".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(link.url_without_fragment(), "https://example.com/");
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
        assert_eq!(link.file_path().unwrap(), "/path/to/file");
    }

    #[test]
    fn test_is_vcs() {
        let link = Link::new(
            "git+https://example.com/repo.git".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(link.is_vcs());
    }

    #[test]
    fn test_is_yanked() {
        let link = Link::new(
            "https://example.com/".to_string(),
            None,
            Some("yanked".to_string()),
            None,
            None,
            None,
        )
        .unwrap();
        assert!(link.is_yanked());
    }

    #[test]
    fn test_filename() {
        let link = Link::new(
            "https://example.com/path/to/file.txt".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(link.filename().unwrap(), "file.txt");
    }

    #[test]
    fn test_is_wheel() {
        let link = Link::new(
            "https://example.com/path/to/file.whl".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(link.is_wheel());
    }

    #[test]
    fn test_subdirectory() {
        let link = Link::new(
            "https://example.com/#subdirectory=foo/bar".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(link.subdirectory().unwrap(), "foo/bar");
    }

    #[test]
    fn test_hashes() {
        let mut expected = HashMap::new();
        expected.insert("sha256".to_string(), "abc123".to_string());
        let link = Link::new(
            "https://example.com/#sha256=abc123".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(link.hashes().unwrap(), expected);
    }

    #[test]
    fn test_hash_options() {
        let mut expected = HashMap::new();
        expected.insert("sha256".to_string(), vec!["abc123".to_string()]);
        let link = Link::new(
            "https://example.com/#sha256=abc123".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(link.hash_options().unwrap(), expected);
    }
}
