use std::{
    collections::BTreeSet,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use futures_util::StreamExt as _;
use governance_domain::PolicyImportTransformationKind;
use reqwest::{Client, StatusCode, header};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::net::lookup_host;
use url::Url;

use crate::{
    AcquiredPolicyArtifact, ConnectorError, PolicySourceConnector, PreparedPolicyArtifact,
};

const ALLOWED_CONTENT_TYPES: &[&str] = &[
    "application/pdf",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "text/plain",
    "text/markdown",
    "text/html",
    "application/xhtml+xml",
];

#[derive(Clone, Debug)]
pub struct SafeFetchConfig {
    pub max_bytes: usize,
    pub max_redirects: usize,
    pub connect_timeout: Duration,
    pub response_timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct SafeUrlFetcher {
    config: SafeFetchConfig,
}

impl SafeUrlFetcher {
    /// Creates a URL fetcher that enforces the supplied transport limits.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client configuration is invalid.
    pub fn new(config: SafeFetchConfig) -> Result<Self, ConnectorError> {
        Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(config.connect_timeout)
            .timeout(config.response_timeout)
            .build()
            .map_err(|_| ConnectorError::terminal("download_failed"))?;
        Ok(Self { config })
    }

    /// Fetches a public HTTPS policy source with DNS, redirect, type, and size validation.
    ///
    /// # Errors
    ///
    /// Returns a classified connector error for unsafe URLs, transport failures, unsupported
    /// media types, redirect exhaustion, and oversized responses.
    #[allow(clippy::too_many_lines)]
    pub async fn fetch(&self, input: &str) -> Result<AcquiredPolicyArtifact, ConnectorError> {
        let mut current = parse_public_https_url(input)?;
        let mut redirects = Vec::new();
        for _ in 0..=self.config.max_redirects {
            let addresses = resolve_public(&current).await?;
            let host = current
                .host_str()
                .ok_or_else(|| ConnectorError::terminal("unsafe_url"))?;
            let port = current
                .port_or_known_default()
                .ok_or_else(|| ConnectorError::terminal("unsafe_url"))?;
            let pinned = SocketAddr::new(
                *addresses
                    .first()
                    .ok_or_else(|| ConnectorError::terminal("unsafe_url"))?,
                port,
            );
            let client = Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(self.config.connect_timeout)
                .timeout(self.config.response_timeout)
                .resolve(host, pinned)
                .build()
                .map_err(|_| ConnectorError::terminal("download_failed"))?;
            let response = client
                .get(current.clone())
                .send()
                .await
                .map_err(|_| ConnectorError::retryable("download_failed"))?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| ConnectorError::terminal("download_failed"))?;
                redirects.push(redacted_url(&current));
                current = parse_public_https_url(
                    current
                        .join(location)
                        .map_err(|_| ConnectorError::terminal("unsafe_url"))?
                        .as_str(),
                )?;
                continue;
            }
            if response.status() == StatusCode::TOO_MANY_REQUESTS
                || response.status().is_server_error()
            {
                return Err(ConnectorError::retryable("download_failed"));
            }
            if !response.status().is_success() {
                return Err(ConnectorError::terminal("download_failed"));
            }
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .map(|value| value.trim().to_ascii_lowercase())
                .ok_or_else(|| ConnectorError::terminal("unsupported_remote_type"))?;
            if !ALLOWED_CONTENT_TYPES.contains(&content_type.as_str()) {
                return Err(ConnectorError::terminal("unsupported_remote_type"));
            }
            if response
                .content_length()
                .is_some_and(|length| length > self.config.max_bytes as u64)
            {
                return Err(ConnectorError::terminal("remote_too_large"));
            }
            let raw_content = bounded_body(response, self.config.max_bytes).await?;
            let prepared = if matches!(content_type.as_str(), "text/html" | "application/xhtml+xml")
            {
                let text = html2text::from_read(raw_content.as_slice(), 100)
                    .map_err(|_| ConnectorError::terminal("unsupported_remote_type"))?;
                if text.len() > self.config.max_bytes {
                    return Err(ConnectorError::terminal("remote_too_large"));
                }
                Some(PreparedPolicyArtifact {
                    kind: PolicyImportTransformationKind::HtmlToText,
                    processor: "html2text".to_owned(),
                    processor_version: env!("CARGO_PKG_VERSION").to_owned(),
                    mime_type: "text/plain".to_owned(),
                    content: text.into_bytes(),
                    metadata: json!({ "redirect_chain": redirects }),
                })
            } else {
                None
            };
            let external_revision = format!("{:x}", Sha256::digest(&raw_content));
            let title = current
                .path_segments()
                .and_then(Iterator::last)
                .filter(|value| !value.is_empty())
                .unwrap_or("Policy source")
                .to_owned();
            return Ok(AcquiredPolicyArtifact {
                external_revision,
                external_modified_at: None,
                canonical_url: Some(redacted_url(&current)),
                title: title.clone(),
                original_filename: Some(title),
                declared_mime_type: Some(content_type),
                raw_content,
                prepared,
            });
        }
        Err(ConnectorError::terminal("unsafe_url"))
    }
}

#[async_trait::async_trait]
impl PolicySourceConnector for SafeUrlFetcher {
    async fn acquire(
        &self,
        external_item_id: &str,
    ) -> Result<AcquiredPolicyArtifact, ConnectorError> {
        self.fetch(external_item_id).await
    }
}

fn parse_public_https_url(input: &str) -> Result<Url, ConnectorError> {
    let url = Url::parse(input).map_err(|_| ConnectorError::terminal("unsafe_url"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || url.fragment().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return Err(ConnectorError::terminal("unsafe_url"));
    }
    Ok(url)
}

async fn resolve_public(url: &Url) -> Result<Vec<IpAddr>, ConnectorError> {
    let host = url
        .host_str()
        .ok_or_else(|| ConnectorError::terminal("unsafe_url"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| ConnectorError::terminal("unsafe_url"))?;
    let answers = lookup_host((host, port))
        .await
        .map_err(|_| ConnectorError::retryable("download_failed"))?;
    let addresses: BTreeSet<_> = answers.map(|address| address.ip()).collect();
    if addresses.is_empty() || addresses.iter().any(|address| !is_public_ip(*address)) {
        return Err(ConnectorError::terminal("unsafe_url"));
    }
    Ok(addresses.into_iter().collect())
}

#[must_use]
pub fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.octets()[0] == 0
                || ip.octets()[0] >= 224)
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(mapped));
            }
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.segments()[0] & 0xffc0 == 0xfe80
                || ip.segments()[0] & 0xff00 == 0xff00)
        }
    }
}

pub(crate) async fn bounded_body(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, ConnectorError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ConnectorError::retryable("download_failed"))?;
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ConnectorError::terminal("remote_too_large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Downloads one already-authorized provider redirect without forwarding credentials.
///
/// The redirect target receives the same public-address validation and DNS pinning as a
/// user-entered URL, but media validation remains the provider adapter's responsibility.
///
/// # Errors
///
/// Returns a classified connector error when the target is unsafe, unavailable, unsuccessful,
/// redirects again, or exceeds the byte limit.
pub async fn bounded_public_download(
    input: &str,
    max_bytes: usize,
    connect_timeout: Duration,
    response_timeout: Duration,
) -> Result<Vec<u8>, ConnectorError> {
    let url = parse_public_https_url(input)?;
    let addresses = resolve_public(&url).await?;
    let host = url
        .host_str()
        .ok_or_else(|| ConnectorError::terminal("unsafe_url"))?;
    let pinned = SocketAddr::new(
        *addresses
            .first()
            .ok_or_else(|| ConnectorError::terminal("unsafe_url"))?,
        443,
    );
    let response = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(connect_timeout)
        .timeout(response_timeout)
        .resolve(host, pinned)
        .build()
        .map_err(|_| ConnectorError::terminal("download_failed"))?
        .get(url)
        .send()
        .await
        .map_err(|_| ConnectorError::retryable("download_failed"))?;
    if !response.status().is_success() {
        return Err(classify_status(response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(ConnectorError::terminal("remote_too_large"));
    }
    bounded_body(response, max_bytes).await
}

pub(crate) fn classify_status(status: StatusCode) -> ConnectorError {
    match status.as_u16() {
        401 => ConnectorError::reauthorize(),
        403 => ConnectorError::terminal("permission_denied"),
        404 => ConnectorError::terminal("remote_deleted"),
        409 => ConnectorError::terminal("remote_conflict"),
        429 => ConnectorError::retryable("provider_rate_limited"),
        _ if status.is_server_error() => ConnectorError::retryable("provider_unavailable"),
        _ => ConnectorError::terminal("download_failed"),
    }
}

fn redacted_url(url: &Url) -> String {
    let mut safe = url.clone();
    safe.set_query(None);
    safe.set_fragment(None);
    safe.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_special_and_mapped_addresses_are_rejected() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "192.0.2.1",
            "0.0.0.0",
            "224.0.0.1",
            "::1",
            "fe80::1",
            "fc00::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(
                !is_public_ip(address.parse().expect("test address should parse")),
                "{address}"
            );
        }
        assert!(is_public_ip("8.8.8.8".parse().expect("public IPv4")));
        assert!(is_public_ip(
            "2606:4700:4700::1111".parse().expect("public IPv6")
        ));
    }

    #[test]
    fn public_url_contract_rejects_credentials_fragments_and_non_https() {
        for url in [
            "http://example.com/policy",
            "https://user@example.com/policy",
            "https://example.com:8443/policy",
            "https://example.com/policy#fragment",
        ] {
            assert!(parse_public_https_url(url).is_err(), "{url}");
        }
        assert!(parse_public_https_url("https://example.com/policy?q=1").is_ok());
    }
}
