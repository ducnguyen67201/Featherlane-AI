use std::time::Duration;

use reqwest::{Client, header};
use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use time::OffsetDateTime;
use url::Url;

use crate::{
    AcquiredPolicyArtifact, ConnectorError, OAuthProof, PolicySourceConnector,
    bounded_public_download,
    safe_fetch::{bounded_body, classify_status},
};

pub const MICROSOFT_GRAPH_SCOPES: &str =
    "offline_access openid profile User.Read Files.Read.All Sites.Read.All";

#[derive(Clone, Debug)]
pub struct MicrosoftGraphClient {
    client_id: String,
    #[allow(dead_code)]
    client_secret: SecretString,
    redirect_uri: Url,
}

impl MicrosoftGraphClient {
    #[must_use]
    pub fn new(client_id: String, client_secret: SecretString, redirect_uri: Url) -> Self {
        Self {
            client_id,
            client_secret,
            redirect_uri,
        }
    }

    /// Builds a Microsoft OAuth authorization URL with state and PKCE protection.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider authorization endpoint cannot be constructed.
    pub fn authorization_url(&self, proof: &OAuthProof) -> Result<Url, ConnectorError> {
        let mut url = Url::parse("https://login.microsoftonline.com/common/oauth2/v2.0/authorize")
            .map_err(|_| ConnectorError::terminal("oauth_denied"))?;
        url.query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", self.redirect_uri.as_str())
            .append_pair("response_type", "code")
            .append_pair("response_mode", "query")
            .append_pair("scope", MICROSOFT_GRAPH_SCOPES)
            .append_pair("state", &proof.state)
            .append_pair("code_challenge", &proof.pkce_challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url)
    }
}

#[derive(Clone, Debug)]
pub struct MicrosoftGraphSourceClient {
    http: Client,
    access_token: SecretString,
    max_bytes: usize,
    graph_base: Url,
}

impl MicrosoftGraphSourceClient {
    /// Creates a bounded Microsoft Graph source client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be configured.
    pub fn new(access_token: SecretString, max_bytes: usize) -> Result<Self, ConnectorError> {
        let graph_base = Url::parse("https://graph.microsoft.com/v1.0/")
            .map_err(|_| ConnectorError::terminal("provider_unavailable"))?;
        Self::from_base(access_token, max_bytes, graph_base)
    }

    fn from_base(
        access_token: SecretString,
        max_bytes: usize,
        graph_base: Url,
    ) -> Result<Self, ConnectorError> {
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| ConnectorError::terminal("provider_unavailable"))?;
        Ok(Self {
            http,
            access_token,
            max_bytes,
            graph_base,
        })
    }

    fn item_url(&self, external_item_id: &str) -> Result<Url, ConnectorError> {
        let (drive_id, item_id) = external_item_id
            .split_once(':')
            .filter(|(drive, item)| !drive.is_empty() && !item.is_empty())
            .ok_or_else(|| ConnectorError::terminal("invalid_provider_item"))?;
        let mut url = self.graph_base.clone();
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| ConnectorError::terminal("provider_unavailable"))?;
        segments.pop_if_empty();
        for segment in ["drives", drive_id, "items", item_id] {
            segments.push(segment);
        }
        drop(segments);
        Ok(url)
    }

    async fn metadata(&self, external_item_id: &str) -> Result<GraphDriveItem, ConnectorError> {
        let response = self
            .http
            .get(self.item_url(external_item_id)?)
            .bearer_auth(self.access_token.expose_secret())
            .send()
            .await
            .map_err(|_| ConnectorError::retryable("provider_unavailable"))?;
        if !response.status().is_success() {
            return Err(classify_status(response.status()));
        }
        response
            .json()
            .await
            .map_err(|_| ConnectorError::terminal("provider_response_invalid"))
    }

    async fn content(&self, external_item_id: &str) -> Result<Vec<u8>, ConnectorError> {
        let mut url = self.item_url(external_item_id)?;
        url.path_segments_mut()
            .map_err(|()| ConnectorError::terminal("provider_unavailable"))?
            .push("content");
        let response = self
            .http
            .get(url)
            .bearer_auth(self.access_token.expose_secret())
            .send()
            .await
            .map_err(|_| ConnectorError::retryable("provider_unavailable"))?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| ConnectorError::terminal("provider_response_invalid"))?;
            return bounded_public_download(
                location,
                self.max_bytes,
                Duration::from_secs(5),
                Duration::from_secs(30),
            )
            .await;
        }
        if !response.status().is_success() {
            return Err(classify_status(response.status()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > self.max_bytes as u64)
        {
            return Err(ConnectorError::terminal("remote_too_large"));
        }
        bounded_body(response, self.max_bytes).await
    }
}

#[async_trait::async_trait]
impl PolicySourceConnector for MicrosoftGraphSourceClient {
    async fn acquire(
        &self,
        external_item_id: &str,
    ) -> Result<AcquiredPolicyArtifact, ConnectorError> {
        let item = self.metadata(external_item_id).await?;
        let mime_type = item
            .file
            .and_then(|file| file.mime_type)
            .or_else(|| mime_from_name(&item.name).map(ToOwned::to_owned))
            .filter(|mime| {
                matches!(
                    mime.as_str(),
                    "application/pdf"
                        | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                        | "text/plain"
                        | "text/markdown"
                )
            })
            .ok_or_else(|| ConnectorError::terminal("unsupported_remote_type"))?;
        let raw_content = self.content(external_item_id).await?;
        let modified = item.last_modified_date_time.as_deref().and_then(|value| {
            OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
        });
        let external_revision = item.e_tag.or(item.c_tag).unwrap_or_else(|| {
            format!(
                "modified:{}",
                item.last_modified_date_time.as_deref().unwrap_or("unknown")
            )
        });
        Ok(AcquiredPolicyArtifact {
            external_revision,
            external_modified_at: modified,
            canonical_url: item.web_url,
            title: item.name.clone(),
            original_filename: Some(item.name),
            declared_mime_type: Some(mime_type),
            raw_content,
            prepared: None,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphDriveItem {
    name: String,
    web_url: Option<String>,
    e_tag: Option<String>,
    c_tag: Option<String>,
    last_modified_date_time: Option<String>,
    file: Option<GraphFileFacet>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphFileFacet {
    mime_type: Option<String>,
}

fn mime_from_name(name: &str) -> Option<&'static str> {
    let extension = std::path::Path::new(name).extension()?;
    if extension.eq_ignore_ascii_case("pdf") {
        Some("application/pdf")
    } else if extension.eq_ignore_ascii_case("docx") {
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
    } else if extension.eq_ignore_ascii_case("txt") {
        Some("text/plain")
    } else if extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown") {
        Some("text/markdown")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::get,
    };
    use serde_json::json;

    use super::*;

    fn authorized(headers: &HeaderMap) -> bool {
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            == Some("Bearer provider-test-token")
    }

    async fn metadata(headers: HeaderMap) -> impl IntoResponse {
        if !authorized(&headers) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        Json(json!({
            "name": "Refund policy.md",
            "webUrl": "https://contoso.sharepoint.com/policy",
            "eTag": "revision-9",
            "lastModifiedDateTime": "2026-08-14T01:02:03Z",
            "file": { "mimeType": "text/markdown" }
        }))
        .into_response()
    }

    async fn content(headers: HeaderMap) -> impl IntoResponse {
        if !authorized(&headers) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        ([("content-type", "text/markdown")], "# Refund policy").into_response()
    }

    #[tokio::test]
    async fn graph_drive_item_uses_authoritative_metadata_and_bounded_content() {
        let app = Router::new()
            .route("/v1.0/drives/{drive_id}/items/{item_id}", get(metadata))
            .route(
                "/v1.0/drives/{drive_id}/items/{item_id}/content",
                get(content),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("provider fixture should bind");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("provider fixture should serve");
        });
        let client = MicrosoftGraphSourceClient::from_base(
            SecretString::from("provider-test-token"),
            1024,
            Url::parse(&format!("http://{address}/v1.0/")).expect("fixture URL"),
        )
        .expect("client should build");

        let artifact = client
            .acquire("drive-1:item-1")
            .await
            .expect("drive item should import");

        assert_eq!(artifact.raw_content, b"# Refund policy");
        assert_eq!(artifact.external_revision, "revision-9");
        assert_eq!(
            artifact.declared_mime_type.as_deref(),
            Some("text/markdown")
        );
        server.abort();
    }
}
