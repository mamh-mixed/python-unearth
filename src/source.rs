use crate::error::ErrorKind;
use crate::session::PyPISession;
use crate::{error::Error, link::Link};
use lazy_static::lazy_static;
use mime_guess;
use scraper::{Html, Selector};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use url::Url;

use crate::link::DistMetadata;

lazy_static! {
    pub static ref ARCHIVE_EXTENSIONS: [&'static str; 10] = [
        ".zip",
        ".whl",
        ".tar.bz2",
        ".tbz",
        ".tar.xz",
        ".txz",
        ".tlz",
        ".tar.lz",
        ".tar.lzma",
        ".tar.gz",
    ];
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Yanked {
    No(bool),
    Yes(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct PackageFile {
    data_dist_info_metadata: Option<DistMetadata>,
    hashes: Option<HashMap<String, String>>,
    requires_python: Option<String>,
    url: Option<String>,
    yanked: Yanked,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct Response {
    files: Vec<PackageFile>,
}

enum PyPIResponse {
    Html(String),
    Json(Response),
}

pub fn collect_links(
    client: &PyPISession,
    source: &Link,
    expand: bool,
) -> Result<Vec<Link>, Error> {
    log::debug!("Collecting links from {}", source);
    let mut collected: Vec<Link> = vec![];
    if source.is_file() {
        let path = PathBuf::from(source.file_path().unwrap());
        if path.is_dir() {
            if expand {
                for entry in path.read_dir()? {
                    let subpath = entry?.path();
                    let file_url = Url::from_file_path(subpath)
                        .map_err(|_| {
                            Error::new(ErrorKind::IOError, "Invalid file URL".to_string())
                        })?
                        .to_string();
                    let file_link = Link::from_str(file_url.as_str())?;
                    if is_html_file(file_url.as_str()) {
                        collected.extend(collect_links_from_page(client, &file_link)?);
                    } else {
                        collected.push(file_link);
                    }
                }
            } else {
                let index = path.join("index.html");
                let file_url = Url::from_file_path(index)
                    .map_err(|_| {
                        Error::new(
                            crate::error::ErrorKind::IOError,
                            "Invalid file URL".to_string(),
                        )
                    })?
                    .to_string();
                let file_link = Link::from_str(file_url.as_str())?;
                collected.extend(collect_links_from_page(client, &file_link)?)
            }
        } else {
            collected.extend(collect_links_from_page(client, source)?);
        }
    } else {
        collected.extend(collect_links_from_page(client, source)?);
    }
    Ok(collected)
}

fn is_html_file(file_url: &str) -> bool {
    let mime_type = mime_guess::from_path(file_url).first_or_octet_stream();
    mime_type == mime_guess::mime::TEXT_HTML
}

fn collect_links_from_page(client: &PyPISession, source: &Link) -> Result<Vec<Link>, Error> {
    let content = if source.is_file() {
        PyPIResponse::Html(std::fs::read_to_string(source.file_path()?)?)
    } else {
        match get_pypi_response(client, &source) {
            Err(e) if e.kind == ErrorKind::CollectError => {
                log::warn!("Failed to collect links from {}: {}", source, e);
                return Ok(vec![]);
            }
            Err(e) => return Err(e),
            Ok(response) => response,
        }
    };
    let from_link = source.url_without_fragment();
    match content {
        PyPIResponse::Html(html) => parse_links_from_html(html, from_link.as_str()),
        PyPIResponse::Json(json) => parse_links_from_json(json, from_link.as_str()),
    }
}

fn get_pypi_response(client: &PyPISession, source: &Link) -> Result<PyPIResponse, Error> {
    if is_archive_file(&source.filename()) {
        ensure_index_response(client, source)?;
    }

    let accept_header = "application/vnd.pypi.simple.v1+json, \
        application/vnd.pypi.simple.v1+html; q=0.1, \
        text/html; q=0.01";
    let response = client
        .get(source.normalized.as_str())
        .header("Accept", accept_header)
        .header("Cache-Control", "max-age=0")
        .send()?;

    check_for_status(&response)?;

    match response.headers().get("Content-Type").map(|v| v.to_str()) {
        Some(Ok("text/html")) | Some(Ok("application/vnd.pypi.simple.v1+html")) => {
            Ok(PyPIResponse::Html(response.text()?))
        }
        Some(Ok("application/vnd.pypi.simple.v1+json")) => Ok(PyPIResponse::Json(response.json()?)),
        Some(Err(_)) => Err(Error::new(
            ErrorKind::CollectError,
            "Invalid Content-Type header".to_string(),
        )),
        _ => Err(Error::new(
            ErrorKind::CollectError,
            "Unsupported Content-Type header".to_string(),
        )),
    }
}

fn parse_links_from_html(html: String, from_url: &str) -> Result<Vec<Link>, Error> {
    let base_url = Url::parse(from_url).unwrap();
    let document = Html::parse_document(html.as_str());
    let selector = Selector::parse("a").unwrap();
    let links = document
        .select(&selector)
        .filter_map(|element| {
            let href = element.value().attr("href")?;
            let url = base_url.join(href).ok()?.to_string();
            let dist_metadata =
                element
                    .value()
                    .attr("data-metadata")
                    .map(|s| match s.split_once('=') {
                        Some((name, value)) => {
                            let mut metadata = HashMap::new();
                            metadata.insert(name.to_string(), value.to_string());
                            DistMetadata::Hashes(metadata)
                        }
                        None => DistMetadata::Enabled(true),
                    });
            Link::new(
                url,
                Some(from_url.to_string()),
                element.value().attr("data-yanked").map(|s| s.to_string()),
                element
                    .value()
                    .attr("data-requires-python")
                    .map(|s| s.to_string()),
                None,
                dist_metadata,
            )
            .ok()
        })
        .collect();
    Ok(links)
}

fn parse_links_from_json(json: Response, from_url: &str) -> Result<Vec<Link>, Error> {
    let base_url = Url::parse(from_url).unwrap();
    let links = json
        .files
        .into_iter()
        .filter_map(|file| {
            let url = base_url.join(file.url?.as_str()).unwrap().to_string();
            Link::new(
                url,
                Some(from_url.to_string()),
                match file.yanked {
                    Yanked::Yes(reason) => Some(reason),
                    _ => None,
                },
                file.requires_python,
                file.hashes,
                file.data_dist_info_metadata,
            )
            .ok()
        })
        .collect();
    Ok(links)
}

fn is_archive_file(name: &str) -> bool {
    let ext = Path::new(name)
        .extension()
        .map(|ext| ext.to_ascii_lowercase());
    ext.map_or(false, |ext| {
        ARCHIVE_EXTENSIONS.contains(&ext.to_string_lossy().as_ref())
    })
}

/// If the URL looks like a file, send a HEAD request to ensure
/// the link is an HTML page to avoid downloading a large file.
fn ensure_index_response(client: &PyPISession, source: &Link) -> Result<(), Error> {
    if source.parsed.scheme() != "http" && source.parsed.scheme() != "https" {
        return Err(Error::new(
            ErrorKind::CollectError,
            "NotHTTP: the file looks like an archive but its content-type \
             cannot be checked by a HEAD request."
                .to_string(),
        ));
    }
    let resp = client.head(source.normalized.as_str()).send()?;
    check_for_status(&resp)
}

fn check_for_status(resp: &reqwest::blocking::Response) -> Result<(), Error> {
    let reason = resp
        .status()
        .canonical_reason()
        .unwrap_or_else(|| "Unknown");
    let status = resp.status().as_u16();

    if status >= 500 {
        Err(Error::new(
            ErrorKind::CollectError,
            format!("Server Error({}): {}", resp.status().as_u16(), reason),
        ))
    } else if status >= 400 {
        Err(Error::new(
            ErrorKind::CollectError,
            format!("Client Error({}): {}", resp.status().as_u16(), reason),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_links_from_html() {
        let client = PyPISession::new();
        let source = Link::from_str("https://pypi.org/simple/cacheyou/").unwrap();
        let links = collect_links(&client, &source, false).unwrap();
        assert!(links.len() > 0);
        let last_link = links.last().unwrap();
        assert!(last_link.dist_metadata.is_some());
        assert!(last_link.requires_python.is_some());
    }
}
