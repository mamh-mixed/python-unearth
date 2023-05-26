use crate::{Error, ErrorKind};

#[derive(Debug)]
pub struct PyPISession {
    client: reqwest::blocking::Client,
    trusted_host_ports: Vec<(String, Option<u16>)>,
}

impl PyPISession {
    pub fn new() -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
            trusted_host_ports: vec![],
        }
    }

    pub fn add_trusted_host(&mut self, host: &str) -> Result<(), Error> {
        let url = build_url_from_netloc(host)?;
        self.trusted_host_ports
            .push((url.host_str().unwrap().to_string(), url.port()));
        Ok(())
    }

    pub fn get(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client.get(url)
    }

    pub fn head(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client.head(url)
    }

    pub fn post(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client.post(url)
    }

    pub fn put(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client.put(url)
    }

    pub fn patch(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client.patch(url)
    }

    pub fn delete(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client.delete(url)
    }
}

fn build_url_from_netloc(netloc: &str) -> Result<url::Url, Error> {
    let full_url =
        if netloc.matches(':').count() >= 2 && !netloc.contains('@') && !netloc.contains('[') {
            // It must be a bare IPv6 address, so wrap it with brackets.
            format!("https://[{}]", netloc)
        } else {
            format!("https://{}", netloc)
        };

    url::Url::parse(&full_url)
        .map_err(|_| Error::new(ErrorKind::ValueError, format!("Invalid host: {netloc}")))
}
