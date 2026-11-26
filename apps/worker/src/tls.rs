//! Mandatory TLS verification for Hub links: Mozilla roots plus an optional
//! operator-supplied `MYRLET_HUB_CA_FILE` PEM bundle. No insecure mode exists.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rustls::RootCertStore;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::CertificateDer;
use url::Url;

/// Operator-supplied extra CA bundle trusted in addition to the Mozilla roots.
pub const ENV_HUB_CA_FILE: &str = "MYRLET_HUB_CA_FILE";

/// Bound every Hub HTTP call so a stalled Hub cannot wedge reporting forever.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Failure to prepare verified Hub transport.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("TLS configuration failed: {0}")]
    Rustls(#[from] rustls::Error),
    #[error("hub URL {url:?} is unusable: {message}")]
    Url { url: String, message: String },
    #[error("Hub CA bundle {path:?} is unusable: {message}")]
    CaBundle { path: PathBuf, message: String },
    #[error("HTTP client setup failed: {message}")]
    Http { message: String },
}

/// `MYRLET_HUB_CA_FILE`, when the operator set a non-empty value.
pub fn ca_file_from_env() -> Option<PathBuf> {
    std::env::var_os(ENV_HUB_CA_FILE)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Build the rustls client configuration: Mozilla roots plus `ca_file` when given.
pub fn build_tls_config(ca_file: Option<&Path>) -> Result<Arc<rustls::ClientConfig>, TlsError> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = ca_file {
        for certificate in read_ca_bundle(path)? {
            roots.add(certificate)?;
        }
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Build the reqwest client used for Pairing and Progress/Finish reporting. Certificate
/// verification is always on; `ca_file` only *adds* a trusted root.
pub fn build_http_client(ca_file: Option<&Path>) -> Result<reqwest::Client, TlsError> {
    let mut builder = reqwest::Client::builder().timeout(HTTP_TIMEOUT);
    if let Some(path) = ca_file {
        let bytes = std::fs::read(path).map_err(|error| ca_error(path, error))?;
        let certificates =
            reqwest::Certificate::from_pem_bundle(&bytes).map_err(|error| ca_error(path, error))?;
        if certificates.is_empty() {
            return Err(no_certificates(path));
        }
        for certificate in certificates {
            builder = builder.add_root_certificate(certificate);
        }
    }
    builder.build().map_err(|error| TlsError::Http {
        message: error.to_string(),
    })
}

/// `http://` → `ws://` and `https://` → `wss://`, targeting the WSS upgrade route.
pub fn hub_ws_url(hub_http_url: &str) -> Result<String, TlsError> {
    let mut url = Url::parse(hub_http_url).map_err(|error| TlsError::Url {
        url: hub_http_url.to_owned(),
        message: error.to_string(),
    })?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        other => {
            return Err(TlsError::Url {
                url: hub_http_url.to_owned(),
                message: format!("unsupported scheme {other:?}; use http or https"),
            })
        }
    };
    url.set_scheme(scheme).map_err(|()| TlsError::Url {
        url: hub_http_url.to_owned(),
        message: "cannot rewrite the Hub URL scheme".to_owned(),
    })?;
    if url.host_str().is_none() {
        return Err(TlsError::Url {
            url: hub_http_url.to_owned(),
            message: "Hub URL has no host".to_owned(),
        });
    }
    let base = url.path().trim_end_matches('/').to_owned();
    url.set_path(&format!("{base}/v1/workers/connect"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn read_ca_bundle(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    let iterator = CertificateDer::pem_file_iter(path).map_err(|error| ca_error(path, error))?;
    let mut certificates = Vec::new();
    for entry in iterator {
        certificates.push(entry.map_err(|error| ca_error(path, error))?);
    }
    if certificates.is_empty() {
        return Err(no_certificates(path));
    }
    Ok(certificates)
}

fn no_certificates(path: &Path) -> TlsError {
    TlsError::CaBundle {
        path: path.to_path_buf(),
        message: "bundle contains no certificates".to_owned(),
    }
}

fn ca_error(path: &Path, error: impl std::fmt::Display) -> TlsError {
    TlsError::CaBundle {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}
