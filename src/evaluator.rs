use std::{collections::HashMap, io::Read, str::FromStr};

use pep440_rs::{Operator, Version, VersionSpecifiers};
use pep508_rs::VersionOrUrl;
use pep_427::WheelName;

use crate::{
    hash, session::PyPISession, source::ARCHIVE_EXTENSIONS, Error, ErrorKind, Link, TargetPython,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NormalizedName(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Package {
    name: String,
    version: Option<Version>,
    link: Link,
}

#[derive(Debug, Clone, Default)]
pub struct FormatControl {
    only_binary: bool,
    no_binary: bool,
}

impl FormatControl {
    pub fn new(only_binary: bool, no_binary: bool) -> Result<Self, Error> {
        if only_binary && no_binary {
            return Err(Error::new(
                ErrorKind::ValueError,
                "Cannot set both `only_binary` and `no_binary`".to_string(),
            ));
        }
        Ok(Self {
            only_binary,
            no_binary,
        })
    }

    pub fn check_format(&self, link: &Link, name: &str) -> Result<(), Error> {
        if self.only_binary && !link.is_wheel() {
            return Err(Error::new(
                ErrorKind::LinkMismatchError,
                format!("Only binaries are allowed for {}", name),
            ));
        }
        if self.no_binary && link.is_wheel() {
            return Err(Error::new(
                ErrorKind::LinkMismatchError,
                format!("Binaries are not allowed for {}", name),
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Evaluator<'a> {
    package_name: &'a str,
    session: &'a PyPISession,
    format_control: FormatControl,
    target_python: TargetPython,
    ignore_compatibility: bool,
    allow_yanked: bool,
    hashes: HashMap<String, Vec<String>>,
}

fn canonicalize_name(name: &str) -> NormalizedName {
    let regex = regex::Regex::new("[-_.]+").unwrap();
    NormalizedName(regex.replace_all(&name.to_lowercase(), "-").into_owned())
}

impl Evaluator<'_> {
    pub fn evaluate_link(&self, mut link: Link) -> Result<Package, Error> {
        self.format_control.check_format(&link, self.package_name)?;
        self.check_yanked(&link)?;
        self.check_requires_python(&link)?;
        let canonical_name = canonicalize_name(self.package_name);
        let version: Version;
        if link.is_wheel() {
            let wheel_name = WheelName::from_str(&link.filename())
                .map_err(|e| Error::new(ErrorKind::LinkMismatchError, e.to_string()))?;
            if canonical_name.0 != wheel_name.distribution {
                return Err(Error::new(
                    ErrorKind::LinkMismatchError,
                    format!(
                        "The package name {} does not match the name {} in the link",
                        self.package_name, wheel_name.distribution
                    ),
                ));
            }
            if !self.ignore_compatibility && !self.target_python.is_compatible(&wheel_name) {
                return Err(Error::new(
                    ErrorKind::LinkMismatchError,
                    format!(
                        "The wheel tags {:?} is not compatible with this Python version",
                        wheel_name.tags
                    ),
                ));
            }
            version = wheel_name.version;
        } else {
            let egg_info = if let Some(egg) = link.egg() {
                Ok::<_, Error>(egg.split('[').next().unwrap().to_string())
            } else {
                let filename = link.filename();
                let (basename, ext) = splitext(&filename);
                if !ARCHIVE_EXTENSIONS.contains(&ext) {
                    return Err(Error::new(
                        ErrorKind::LinkMismatchError,
                        format!("Unsupported file format: {}", link),
                    ));
                }
                Ok(basename.to_owned())
            }?;

            let ver = parse_version_from_egg_info(&egg_info, &canonical_name).ok_or(Error::new(
                ErrorKind::LinkMismatchError,
                format!("Missing version in the filename {}", egg_info),
            ))?;

            version = Version::from_str(ver).map_err(|e| {
                Error::new(
                    ErrorKind::LinkMismatchError,
                    format!("Invalid version: {}", e),
                )
            })?;
        }
        self.check_hash(&mut link)?;
        Ok(Package {
            name: self.package_name.to_string(),
            version: Some(version),
            link,
        })
    }

    fn check_yanked(&self, link: &Link) -> Result<(), Error> {
        if let Some(reason) = &link.yank_reason {
            if !self.allow_yanked {
                return Err(Error::new(
                    ErrorKind::LinkMismatchError,
                    format!("Yanked due to {}", reason),
                ));
            }
        }
        Ok(())
    }

    fn check_requires_python(&self, link: &Link) -> Result<(), Error> {
        if let Some(requires_python) = &link.requires_python {
            if !self.ignore_compatibility {
                let py_ver: Version = self.target_python.py_ver.into();
                let requires_python =
                    VersionSpecifiers::from_str(requires_python).map_err(|e| {
                        Error::new(
                            ErrorKind::LinkMismatchError,
                            format!("Invalid requires-python specifier: {}", e),
                        )
                    })?;
                if !requires_python.contains(&py_ver) {
                    return Err(Error::new(
                        ErrorKind::LinkMismatchError,
                        format!(
                            "The target python version({}) doesn't match the requires-python specifier {}",
                            &py_ver, requires_python
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    fn get_hash(&self, link: &mut Link, hash_name: &str) -> Result<String, Error> {
        let mut resp = self.session.get(&link.normalized).send()?;
        let mut hasher = hash::Hasher::new(hash_name).ok_or(Error::new(
            ErrorKind::LinkMismatchError,
            format!("Unsupported hash algo {}", hash_name),
        ))?;
        let mut buffer = [0; 1024 * 8];
        loop {
            let bytes_read = resp.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        let digest = hasher.hexdigest();
        match link.hashes_map {
            Some(ref mut hashes) => {
                hashes.insert(hash_name.to_string(), digest.clone());
            }
            None => {
                let mut hashes = HashMap::new();
                hashes.insert(hash_name.to_string(), digest.clone());
                link.hashes_map = Some(hashes);
            }
        }
        Ok(digest)
    }

    fn check_hash(&self, link: &mut Link) -> Result<(), Error> {
        if self.hashes.len() == 0 {
            return Ok(());
        }
        if let Some(link_hashes) = link.hashes() {
            for (hash_name, expected) in &self.hashes {
                if let Some(actual) = link_hashes.get(hash_name) {
                    if !expected.contains(&actual) {
                        return Err(hash_mismatch(hash_name, expected, actual));
                    }
                }
            }
        }

        let (hash_name, expected) = self.hashes.iter().next().unwrap();
        let actual = self.get_hash(link, hash_name)?;
        if !expected.contains(&actual) {
            return Err(hash_mismatch(hash_name, expected, &actual));
        }

        Ok(())
    }
}

fn hash_mismatch(hash_name: &str, expected: &Vec<String>, actual: &str) -> Error {
    Error::new(
        ErrorKind::LinkMismatchError,
        format!(
            "Hash mismatch for {}: expected {}, actual {}",
            hash_name,
            expected.join("/"),
            actual
        ),
    )
}

fn splitext(filename: &str) -> (&str, &str) {
    let mut ext = "";
    let mut basename = filename;
    if let Some(pos) = filename.rfind('.') {
        ext = &filename[pos..];
        basename = &filename[..pos];
        if basename.ends_with(".tar") {
            basename = &filename[..pos - 4];
            ext = &filename[pos - 4..];
        }
    }
    (basename, ext)
}

fn parse_version_from_egg_info<'a>(
    egg_info: &'a str,
    canonical_name: &NormalizedName,
) -> Option<&'a str> {
    for (i, c) in egg_info.char_indices() {
        if &canonicalize_name(&egg_info[..i]) == canonical_name && (c == '-' || c == '_') {
            return Some(&egg_info[i + 1..]);
        }
    }
    None
}

pub fn evaluate_package(
    package: Package,
    requirement: &pep508_rs::Requirement,
    allow_prerelease: Option<bool>,
) -> Result<Package, Error> {
    if canonicalize_name(&requirement.name) != canonicalize_name(&package.name) {
        return Err(Error::new(
            ErrorKind::LinkMismatchError,
            format!(
                "Package name mismatch: expected {}, actual {}, skipping",
                requirement.name, package.name
            ),
        ));
    }

    if let Some(version) = &package.version {
        if let Some(VersionOrUrl::VersionSpecifier(spec)) = &requirement.version_or_url {
            if !spec.contains(version) {
                return Err(Error::new(
                    ErrorKind::LinkMismatchError,
                    format!(
                        "Version mismatch: expected {}, actual {}, skipping",
                        spec, version
                    ),
                ));
            }

            let allow_prerelease = allow_prerelease.unwrap_or(spec.iter().any(|s| {
                matches!(
                    s.operator(),
                    Operator::Equal
                        | Operator::ExactEqual
                        | Operator::GreaterThanEqual
                        | Operator::LessThanEqual
                        | Operator::TildeEqual
                ) && s.version().any_prerelease()
            }));
            if version.any_prerelease() && !allow_prerelease {
                return Err(Error::new(
                    ErrorKind::LinkMismatchError,
                    format!(
                        "Prerelease version not permitted: expected {}, actual {}, skipping",
                        spec, version
                    ),
                ));
            }
        }
    };

    Ok(package)
}
