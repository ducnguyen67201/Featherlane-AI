use std::time::Duration;

use governance_domain::PolicyImportTransformationKind;
use reqwest::Client;
use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use url::Url;

use crate::{
    AcquiredPolicyArtifact, ConnectorError, PolicySourceConnector, PreparedPolicyArtifact,
    safe_fetch::{bounded_body, classify_status},
};

pub const NOTION_API_VERSION: &str = "2026-03-11";

#[derive(Clone, Debug)]
pub struct NotionClient {
    client_id: String,
    #[allow(dead_code)]
    client_secret: SecretString,
    redirect_uri: Url,
}

impl NotionClient {
    #[must_use]
    pub fn new(client_id: String, client_secret: SecretString, redirect_uri: Url) -> Self {
        Self {
            client_id,
            client_secret,
            redirect_uri,
        }
    }

    /// Builds a Notion OAuth authorization URL with one-time state.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider authorization endpoint cannot be constructed.
    pub fn authorization_url(&self, state: &str) -> Result<Url, ConnectorError> {
        let mut url = Url::parse("https://api.notion.com/v1/oauth/authorize")
            .map_err(|_| ConnectorError::terminal("oauth_denied"))?;
        url.query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", self.redirect_uri.as_str())
            .append_pair("response_type", "code")
            .append_pair("owner", "user")
            .append_pair("state", state);
        Ok(url)
    }
}

#[derive(Clone, Debug)]
pub struct NotionSourceClient {
    http: Client,
    access_token: SecretString,
    max_bytes: usize,
    api_base: Url,
}

impl NotionSourceClient {
    /// Creates a bounded Notion source client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be configured.
    pub fn new(access_token: SecretString, max_bytes: usize) -> Result<Self, ConnectorError> {
        let api_base = Url::parse("https://api.notion.com/v1/")
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

    fn page_url(&self, page_id: &str) -> Result<Url, ConnectorError> {
        if page_id.trim().is_empty() || page_id.chars().count() > 128 {
            return Err(ConnectorError::terminal("invalid_provider_item"));
        }
        let mut url = self.api_base.clone();
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| ConnectorError::terminal("provider_unavailable"))?;
        segments.pop_if_empty();
        for segment in ["pages", page_id] {
            segments.push(segment);
        }
        drop(segments);
        Ok(url)
    }

    async fn request(&self, url: Url) -> Result<reqwest::Response, ConnectorError> {
        let response = self
            .http
            .get(url)
            .bearer_auth(self.access_token.expose_secret())
            .header("Notion-Version", NOTION_API_VERSION)
            .send()
            .await
            .map_err(|_| ConnectorError::retryable("provider_unavailable"))?;
        if !response.status().is_success() {
            return Err(classify_status(response.status()));
        }
        Ok(response)
    }
}

#[async_trait::async_trait]
impl PolicySourceConnector for NotionSourceClient {
    async fn acquire(
        &self,
        external_item_id: &str,
    ) -> Result<AcquiredPolicyArtifact, ConnectorError> {
        let metadata: NotionPage = self
            .request(self.page_url(external_item_id)?)
            .await?
            .json()
            .await
            .map_err(|_| ConnectorError::terminal("provider_response_invalid"))?;
        let mut markdown_url = self.page_url(external_item_id)?;
        markdown_url
            .path_segments_mut()
            .map_err(|()| ConnectorError::terminal("provider_unavailable"))?
            .push("markdown");
        let response = self.request(markdown_url).await?;
        if response
            .content_length()
            .is_some_and(|length| length > self.max_bytes as u64)
        {
            return Err(ConnectorError::terminal("remote_too_large"));
        }
        let raw_content = bounded_body(response, self.max_bytes).await?;
        let parsed = serde_json::from_slice::<NotionMarkdown>(&raw_content).ok();
        if parsed.as_ref().is_some_and(|value| {
            value.truncated.unwrap_or(false)
                || value
                    .unknown_block_ids
                    .as_ref()
                    .is_some_and(|ids| !ids.is_empty())
        }) {
            return Err(ConnectorError::terminal("notion_content_incomplete"));
        }
        let markdown = parsed.as_ref().map_or_else(
            || String::from_utf8_lossy(&raw_content).into_owned(),
            |value| value.markdown.clone(),
        );
        if markdown.trim().is_empty() {
            return Err(ConnectorError::terminal("notion_content_incomplete"));
        }
        if markdown.len() > self.max_bytes {
            return Err(ConnectorError::terminal("remote_too_large"));
        }
        let title = notion_title(&metadata.properties)
            .unwrap_or_else(|| format!("Notion page {}", metadata.id));
        let modified = metadata.last_edited_time.as_deref().and_then(|value| {
            OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
        });
        Ok(AcquiredPolicyArtifact {
            external_revision: metadata
                .last_edited_time
                .clone()
                .unwrap_or_else(|| metadata.id.clone()),
            external_modified_at: modified,
            canonical_url: metadata.url,
            title,
            original_filename: Some(format!("{}.md", metadata.id)),
            declared_mime_type: Some("application/json".to_owned()),
            raw_content,
            prepared: Some(PreparedPolicyArtifact {
                kind: PolicyImportTransformationKind::NotionMarkdown,
                processor: "notion-enhanced-markdown".to_owned(),
                processor_version: NOTION_API_VERSION.to_owned(),
                mime_type: "text/markdown".to_owned(),
                content: markdown.into_bytes(),
                metadata: json!({ "page_id": metadata.id, "complete": true }),
            }),
        })
    }
}

#[derive(Debug, Deserialize)]
struct NotionPage {
    id: String,
    url: Option<String>,
    last_edited_time: Option<String>,
    #[serde(default)]
    properties: Value,
}

#[derive(Debug, Deserialize)]
struct NotionMarkdown {
    markdown: String,
    truncated: Option<bool>,
    unknown_block_ids: Option<Vec<String>>,
}

fn notion_title(properties: &Value) -> Option<String> {
    let title = properties
        .as_object()?
        .values()
        .find_map(|property| property.get("title").and_then(Value::as_array))?
        .iter()
        .filter_map(|part| part.get("plain_text").and_then(Value::as_str))
        .collect::<String>()
        .trim()
        .to_owned();
    (!title.is_empty()).then_some(title)
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

    fn authorized(headers: &HeaderMap) -> bool {
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            == Some("Bearer provider-test-token")
            && headers
                .get("notion-version")
                .and_then(|value| value.to_str().ok())
                == Some(NOTION_API_VERSION)
    }

    async fn metadata(Path(page_id): Path<String>, headers: HeaderMap) -> impl IntoResponse {
        if !authorized(&headers) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        Json(json!({
            "id": page_id,
            "url": "https://www.notion.so/policy",
            "last_edited_time": "2026-08-14T02:03:04Z",
            "properties": {
                "Name": { "title": [{ "plain_text": "Travel policy" }] }
            }
        }))
        .into_response()
    }

    async fn markdown(Path(page_id): Path<String>, headers: HeaderMap) -> impl IntoResponse {
        if !authorized(&headers) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if page_id == "incomplete" {
            Json(json!({
                "markdown": "partial",
                "truncated": true,
                "unknown_block_ids": ["missing-child"]
            }))
            .into_response()
        } else {
            Json(json!({
                "markdown": "# Travel policy\nManager approval is required.",
                "truncated": false,
                "unknown_block_ids": []
            }))
            .into_response()
        }
    }

    #[tokio::test]
    async fn notion_markdown_is_transformed_only_when_provider_reports_complete_content() {
        let app = Router::new()
            .route("/v1/pages/{page_id}", get(metadata))
            .route("/v1/pages/{page_id}/markdown", get(markdown));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("provider fixture should bind");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("provider fixture should serve");
        });
        let client = NotionSourceClient::from_base(
            SecretString::from("provider-test-token"),
            2048,
            Url::parse(&format!("http://{address}/v1/")).expect("fixture URL"),
        )
        .expect("client should build");

        let artifact = client
            .acquire("complete")
            .await
            .expect("complete Notion page should import");
        assert_eq!(artifact.title, "Travel policy");
        assert_eq!(
            artifact
                .prepared
                .as_ref()
                .map(|value| value.content.as_slice()),
            Some(b"# Travel policy\nManager approval is required.".as_slice())
        );

        let error = client
            .acquire("incomplete")
            .await
            .expect_err("truncated Notion page must fail closed");
        assert_eq!(error.code, "notion_content_incomplete");
        assert_eq!(error.retry, crate::ConnectorRetry::Never);
        server.abort();
    }
}
