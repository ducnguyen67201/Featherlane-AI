#![allow(clippy::result_large_err)]

use axum::{
    http::{HeaderMap, StatusCode},
    response::Response,
};
use governance_config::SourceConnectorConfig;
use secrecy::ExposeSecret as _;
use subtle::ConstantTimeEq as _;

use crate::loco_app::problem;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsoleActor {
    pub id: String,
}

pub fn authenticate(
    headers: &HeaderMap,
    config: &SourceConnectorConfig,
) -> Result<ConsoleActor, Response> {
    let configured = config.console_api_key.expose_secret().as_bytes();
    if configured.is_empty() {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the governance console boundary is not configured",
        ));
    }
    let supplied = headers
        .get("x-featherlane-console-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .as_bytes();
    if supplied.len() != configured.len() || supplied.ct_eq(configured).unwrap_u8() != 1 {
        return Err(problem(
            StatusCode::UNAUTHORIZED,
            "console authentication failed",
        ));
    }
    let actor_id = headers
        .get("x-featherlane-actor-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .unwrap_or_default();
    if actor_id.is_empty()
        || actor_id.chars().count() > 320
        || actor_id.chars().any(char::is_control)
    {
        return Err(problem(
            StatusCode::UNAUTHORIZED,
            "a valid console actor is required",
        ));
    }
    Ok(ConsoleActor {
        id: actor_id.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use governance_config::SourceConnectorConfig;
    use secrecy::SecretString;
    use url::Url;

    use super::*;

    fn config() -> SourceConnectorConfig {
        SourceConnectorConfig {
            console_api_key: SecretString::from("server-key".to_owned()),
            encryption_keys: BTreeMap::new(),
            active_key_version: None,
            callback_base_url: Url::parse("http://localhost:3000").expect("valid URL"),
            max_items_per_batch: 25,
            max_batch_bytes: 100,
            max_redirects: 5,
            oauth_state_ttl_seconds: 600,
            connect_timeout_seconds: 5,
            response_timeout_seconds: 30,
            google: None,
            microsoft: None,
            notion: None,
        }
    }

    #[test]
    fn requires_exact_key_and_bounded_actor() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-featherlane-console-key",
            "server-key".parse().expect("header"),
        );
        headers.insert(
            "x-featherlane-actor-id",
            "owner@example.com".parse().expect("header"),
        );
        assert_eq!(
            authenticate(&headers, &config()).expect("authorized").id,
            "owner@example.com"
        );

        headers.insert(
            "x-featherlane-console-key",
            "wrong-key".parse().expect("header"),
        );
        assert!(authenticate(&headers, &config()).is_err());
        headers.remove("x-featherlane-console-key");
        assert!(authenticate(&headers, &config()).is_err());
    }
}
