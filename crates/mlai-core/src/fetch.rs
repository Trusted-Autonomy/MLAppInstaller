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
}

pub struct HttpFetcher {
    pub token: Option<String>,
}

impl Fetcher for HttpFetcher {
    fn fetch(&self, url: &str, dest_zip: &Path) -> Result<(), FetchError> {
        let client = reqwest::blocking::Client::new();
        let mut request = client.get(url);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
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
}

/// Unpacks `zip_path` into `dest_dir`, renaming the archive's single
/// top-level folder to `component_name` so components land as predictable
/// sibling directories (mirrors cinepipe-installer's `Expand-CpBundle`).
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
