use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("HTTP request to {url} failed: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("HTTP request to {url} returned status {status}")]
    Status { url: String, status: u16 },
    #[error("I/O failure at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("zip extraction failed: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("downloaded archive at {0} had no top-level directory")]
    NoTopLevelDir(PathBuf),
}

pub trait Fetcher {
    fn fetch(&self, url: &str, dest_zip: &Path) -> Result<(), FetchError>;

    /// Best-effort identity for the content currently at `url` (an HTTP
    /// `ETag` or `Last-Modified` value), obtained without downloading it.
    /// Lets the pipeline detect that a mutable ref (a branch tip, unlike a
    /// pinned release tag) changed since the last install without trusting
    /// a static `ref` string as if it were a version. Returns `None` when
    /// the server exposes neither header, doesn't support the lookup, or is
    /// unreachable — callers fall back to the manifest's declared version in
    /// that case, so this never fails the install outright.
    fn remote_identity(&self, _url: &str) -> Option<String> {
        None
    }
}

pub struct HttpFetcher {
    pub token: Option<String>,
}

impl HttpFetcher {
    fn authed_request(
        &self,
        client: &reqwest::blocking::Client,
        method: reqwest::Method,
        url: &str,
    ) -> reqwest::blocking::RequestBuilder {
        let mut request = client.request(method, url);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        request
    }
}

impl Fetcher for HttpFetcher {
    fn fetch(&self, url: &str, dest_zip: &Path) -> Result<(), FetchError> {
        let client = reqwest::blocking::Client::new();
        let request = self.authed_request(&client, reqwest::Method::GET, url);
        let response = request.send().map_err(|source| FetchError::Http {
            url: url.to_string(),
            source,
        })?;
        if !response.status().is_success() {
            return Err(FetchError::Status {
                url: url.to_string(),
                status: response.status().as_u16(),
            });
        }
        let bytes = response.bytes().map_err(|source| FetchError::Http {
            url: url.to_string(),
            source,
        })?;
        if let Some(parent) = dest_zip.parent() {
            fs::create_dir_all(parent).map_err(|source| FetchError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(dest_zip, &bytes).map_err(|source| FetchError::Io {
            path: dest_zip.to_path_buf(),
            source,
        })
    }

    fn remote_identity(&self, url: &str) -> Option<String> {
        let client = reqwest::blocking::Client::new();
        let response = self
            .authed_request(&client, reqwest::Method::HEAD, url)
            .send()
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        response
            .headers()
            .get(reqwest::header::ETAG)
            .or_else(|| response.headers().get(reqwest::header::LAST_MODIFIED))
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string())
    }
}

/// Unpacks `zip_path` into `dest_dir`, renaming the archive's single
/// top-level folder to `component_name` so components land as predictable
/// sibling directories.
pub fn unpack_zip(
    zip_path: &Path,
    dest_dir: &Path,
    component_name: &str,
) -> Result<PathBuf, FetchError> {
    let file = fs::File::open(zip_path).map_err(|source| FetchError::Io {
        path: zip_path.to_path_buf(),
        source,
    })?;
    let mut archive = zip::ZipArchive::new(file)?;

    let staging = dest_dir.join(format!(".{component_name}-staging"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|source| FetchError::Io {
            path: staging.clone(),
            source,
        })?;
    }
    archive.extract(&staging)?;

    let top_level = fs::read_dir(&staging)
        .map_err(|source| FetchError::Io {
            path: staging.clone(),
            source,
        })?
        .filter_map(|e| e.ok())
        .find(|e| e.path().is_dir())
        .map(|e| e.path())
        .ok_or_else(|| FetchError::NoTopLevelDir(staging.clone()))?;

    let final_dir = dest_dir.join(component_name);
    if final_dir.exists() {
        fs::remove_dir_all(&final_dir).map_err(|source| FetchError::Io {
            path: final_dir.clone(),
            source,
        })?;
    }
    fs::rename(&top_level, &final_dir).map_err(|source| FetchError::Io {
        path: final_dir.clone(),
        source,
    })?;
    fs::remove_dir_all(&staging).ok();

    Ok(final_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn http_fetcher_downloads_bytes_to_dest() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("GET", "/bundle.zip")
            .with_status(200)
            .with_body(b"fake-zip-bytes")
            .create();

        let dir = tempdir().unwrap();
        let dest = dir.path().join("bundle.zip");
        let fetcher = HttpFetcher { token: None };
        fetcher
            .fetch(&format!("{}/bundle.zip", server.url()), &dest)
            .unwrap();

        assert_eq!(fs::read(&dest).unwrap(), b"fake-zip-bytes");
    }

    #[test]
    fn http_fetcher_errors_on_non_success_status() {
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/missing.zip").with_status(404).create();

        let dir = tempdir().unwrap();
        let dest = dir.path().join("missing.zip");
        let fetcher = HttpFetcher { token: None };
        let err = fetcher
            .fetch(&format!("{}/missing.zip", server.url()), &dest)
            .unwrap_err();

        assert!(matches!(err, FetchError::Status { status: 404, .. }));
    }

    #[test]
    fn remote_identity_returns_the_etag_when_present() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("HEAD", "/bundle.zip")
            .with_status(200)
            .with_header("etag", "\"abc123\"")
            .create();

        let fetcher = HttpFetcher { token: None };
        let identity = fetcher.remote_identity(&format!("{}/bundle.zip", server.url()));

        assert_eq!(identity.as_deref(), Some("\"abc123\""));
    }

    #[test]
    fn remote_identity_falls_back_to_last_modified_when_no_etag() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("HEAD", "/bundle.zip")
            .with_status(200)
            .with_header("last-modified", "Tue, 01 Sep 2026 00:00:00 GMT")
            .create();

        let fetcher = HttpFetcher { token: None };
        let identity = fetcher.remote_identity(&format!("{}/bundle.zip", server.url()));

        assert_eq!(identity.as_deref(), Some("Tue, 01 Sep 2026 00:00:00 GMT"));
    }

    #[test]
    fn remote_identity_is_none_when_server_exposes_neither_header() {
        let mut server = mockito::Server::new();
        let _mock = server.mock("HEAD", "/bundle.zip").with_status(200).create();

        let fetcher = HttpFetcher { token: None };
        let identity = fetcher.remote_identity(&format!("{}/bundle.zip", server.url()));

        assert_eq!(identity, None);
    }

    #[test]
    fn remote_identity_is_none_on_non_success_status() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("HEAD", "/missing.zip")
            .with_status(404)
            .with_header("etag", "\"abc123\"")
            .create();

        let fetcher = HttpFetcher { token: None };
        let identity = fetcher.remote_identity(&format!("{}/missing.zip", server.url()));

        assert_eq!(identity, None);
    }

    #[test]
    fn default_remote_identity_is_none() {
        struct NoOpFetcher;
        impl Fetcher for NoOpFetcher {
            fn fetch(&self, _url: &str, _dest_zip: &Path) -> Result<(), FetchError> {
                Ok(())
            }
        }
        assert_eq!(NoOpFetcher.remote_identity("https://example.com"), None);
    }

    fn build_fixture_zip(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.add_directory("hello-component-main/", options).unwrap();
        zip.start_file("hello-component-main/marker.txt", options)
            .unwrap();
        zip.write_all(b"ok").unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn unpack_zip_renames_top_level_dir_to_component_name() {
        let dir = tempdir().unwrap();
        let zip_path = dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let result_dir = unpack_zip(&zip_path, dir.path(), "hello-component").unwrap();

        assert_eq!(result_dir, dir.path().join("hello-component"));
        assert_eq!(
            fs::read_to_string(result_dir.join("marker.txt")).unwrap(),
            "ok"
        );
    }
}
