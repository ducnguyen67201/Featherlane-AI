use std::time::Duration;

use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use time::OffsetDateTime;
use url::Url;

use crate::{
    AcquiredPolicyArtifact, ConnectorError, OAuthProof, PolicySourceConnector,
    safe_fetch::{bounded_body, classify_status},
};

pub const GOOGLE_DRIVE_SCOPE: &str =
    "openid email profile https://www.googleapis.com/auth/drive.file";

#[derive(Clone, Debug)]
pub struct GoogleDriveClient {
    client_id: String,
    client_secret: SecretString,
    redirect_uri: Url,
}

impl GoogleDriveClient {
    #[must_use]
    pub fn new(client_id: String, client_secret: SecretString, redirect_uri: Url) -> Self {
        Self {
            client_id,
            client_secret,
            redirect_uri,
        }
    }

    /// Builds a Google OAuth authorization URL with state and PKCE protection.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider authorization endpoint cannot be constructed.
    pub fn authorization_url(&self, proof: &OAuthProof) -> Result<Url, ConnectorError> {
        let mut url = Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
            .map_err(|_| ConnectorError::terminal("oauth_denied"))?;
        url.query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", self.redirect_uri.as_str())
            .append_pair("response_type", "code")
            .append_pair("scope", GOOGLE_DRIVE_SCOPE)
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent")
            .append_pair("state", &proof.state)
            .append_pair("code_challenge", &proof.pkce_challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url)
    }

    #[must_use]
    pub fn client_secret(&self) -> &str {
        self.client_secret.expose_secret()
    }
}

const GOOGLE_DOCUMENT_MIME: &str = "application/vnd.google-apps.document";
const DOCX_MIME: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

#[derive(Clone, Debug)]
pub struct GoogleDriveSourceClient {
    http: Client,
    access_token: SecretString,
    max_bytes: usize,
    api_base: Url,
}

impl GoogleDriveSourceClient {
    /// Creates a bounded Google Drive source client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be configured.
    pub fn new(access_token: SecretString, max_bytes: usize) -> Result<Self, ConnectorError> {
        let api_base = Url::parse("https://www.googleapis.com/")
            .map_err(|_| ConnectorError::terminal("provider_unavailable"))?;
        Self::from_base(access_token, max_bytes, api_base)
    }

    fn from_base(
        access_token: SecretString,
        max_bytes: usize,
        api_base: Url,
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
            api_base,
        })
    }

    fn file_url(&self, file_id: &str) -> Result<Url, ConnectorError> {
        let mut url = self.api_base.clone();
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| ConnectorError::terminal("provider_unavailable"))?;
        segments.pop_if_empty();
        for segment in ["drive", "v3", "files", file_id] {
            segments.push(segment);
        }
        drop(segments);
        Ok(url)
    }

    async fn metadata(&self, file_id: &str) -> Result<GoogleFile, ConnectorError> {
        if file_id.trim().is_empty() || file_id.chars().count() > 1024 {
            return Err(ConnectorError::terminal("invalid_provider_item"));
        }
        let mut url = self.file_url(file_id)?;
        url.query_pairs_mut().append_pair(
            "fields",
            "id,name,mimeType,modifiedTime,version,md5Checksum,size,webViewLink",
        );
        let response = self
            .http
            .get(url)
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

    async fn content(
        &self,
        file: &GoogleFile,
    ) -> Result<(Vec<u8>, String, String), ConnectorError> {
        let (parameter, mime_type, filename) = if file.mime_type == GOOGLE_DOCUMENT_MIME {
            (
                ("mimeType", DOCX_MIME),
                DOCX_MIME.to_owned(),
                ensure_extension(&file.name, "docx"),
            )
        } else if matches!(
            file.mime_type.as_str(),
            "application/pdf" | DOCX_MIME | "text/plain" | "text/markdown"
        ) {
            (("alt", "media"), file.mime_type.clone(), file.name.clone())
        } else {
            return Err(ConnectorError::terminal("unsupported_remote_type"));
        };
        let mut url = self.file_url(&file.id)?;
        if file.mime_type == GOOGLE_DOCUMENT_MIME {
            url.path_segments_mut()
                .map_err(|()| ConnectorError::terminal("provider_unavailable"))?
                .push("export");
        }
        url.query_pairs_mut().append_pair(parameter.0, parameter.1);
        let response = self
            .http
            .get(url)
            .bearer_auth(self.access_token.expose_secret())
            .send()
            .await
            .map_err(|_| ConnectorError::retryable("provider_unavailable"))?;
        if response.status() == StatusCode::FORBIDDEN && file.mime_type == GOOGLE_DOCUMENT_MIME {
            return Err(ConnectorError::terminal("google_export_too_large"));
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
        Ok((
            bounded_body(response, self.max_bytes).await?,
            mime_type,
            filename,
        ))
    }
}

#[async_trait::async_trait]
impl PolicySourceConnector for GoogleDriveSourceClient {
    async fn acquire(
        &self,
        external_item_id: &str,
    ) -> Result<AcquiredPolicyArtifact, ConnectorError> {
        let file = self.metadata(external_item_id).await?;
        let (raw_content, mime_type, filename) = self.content(&file).await?;
        let modified = file.modified_time.as_deref().and_then(|value| {
            OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
        });
        let external_revision = format!(
            "{}:{}",
            file.version.as_deref().unwrap_or("unknown"),
            file.modified_time.as_deref().unwrap_or("unknown")
        );
        Ok(AcquiredPolicyArtifact {
            external_revision,
            external_modified_at: modified,
            canonical_url: file.web_view_link,
            title: file.name,
            original_filename: Some(filename),
            declared_mime_type: Some(mime_type),
            raw_content,
            prepared: None,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleFile {
    id: String,
    name: String,
    mime_type: String,
    modified_time: Option<String>,
    version: Option<String>,
    web_view_link: Option<String>,
}

fn ensure_extension(name: &str, extension: &str) -> String {
    if name
        .to_ascii_lowercase()
        .ends_with(&format!(".{extension}"))
    {
        name.to_owned()
    } else {
        format!("{name}.{extension}")
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        extract::Path,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::get,
    };
    use serde_json::json;

    use super::*;

    async fn metadata(Path(file_id): Path<String>, headers: HeaderMap) -> impl IntoResponse {
        if headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            != Some("Bearer provider-test-token")
        {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        Json(json!({
            "id": file_id,
            "name": "Operations policy",
            "mimeType": GOOGLE_DOCUMENT_MIME,
            "modifiedTime": "2026-08-14T00:00:00Z",
            "version": "7",
            "webViewLink": "https://drive.google.com/document/d/doc-1/view"
        }))
        .into_response()
    }

    async fn export(headers: HeaderMap) -> impl IntoResponse {
        if headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            != Some("Bearer provider-test-token")
        {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        ([("content-type", DOCX_MIME)], b"mock-docx".to_vec()).into_response()
    }

    #[tokio::test]
    async fn google_document_metadata_drives_authoritative_docx_export() {
        let app = Router::new()
            .route("/drive/v3/files/{file_id}", get(metadata))
            .route("/drive/v3/files/{file_id}/export", get(export));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("provider fixture should bind");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("provider fixture should serve");
        });
        let client = GoogleDriveSourceClient::from_base(
            SecretString::from("provider-test-token"),
            1024,
            Url::parse(&format!("http://{address}/")).expect("fixture URL"),
        )
        .expect("client should build");

        let artifact = client.acquire("doc-1").await.expect("document export");

        assert_eq!(artifact.raw_content, b"mock-docx");
        assert_eq!(artifact.declared_mime_type.as_deref(), Some(DOCX_MIME));
        assert_eq!(
            artifact.original_filename.as_deref(),
            Some("Operations policy.docx")
        );
        assert_eq!(artifact.external_revision, "7:2026-08-14T00:00:00Z");
        server.abort();
    }
}
