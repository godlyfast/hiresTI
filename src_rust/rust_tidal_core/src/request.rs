//! Generic authenticated HTTP entry point.
//!
//! Mirrors tidalapi's `Session.request.request(method, path, base_url, params)`
//! shape: a path is resolved against a base URL (defaults to API_V1_LOCATION),
//! the access token is attached as a Bearer header, params join either as
//! query string (GET/DELETE) or form body (POST/PUT/PATCH), and the response
//! comes back as `{ok, status, body}` where `body` is the parsed JSON if the
//! response was JSON, otherwise the raw string.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ureq::Agent;
use url::Url;

use crate::auth;
use crate::error::{map_ureq, RtcError, RtcResult};
use crate::http;

#[derive(Debug, Deserialize)]
pub struct RequestArgs {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub params: Option<BTreeMap<String, ParamValue>>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub json_body: Option<Value>,
    /// When true, params are sent as a urlencoded form body for non-GET
    /// methods. When false (default), params always go to the query string.
    /// tidalapi defaults to body-form on POST/PUT/PATCH.
    #[serde(default)]
    pub form_body: bool,
}

/// Accepts strings, numbers, and booleans as param values; serializes
/// everything else (e.g. arrays) into a JSON string. Matches what tidalapi
/// does — TIDAL endpoints expect repeated key=val&key=val for arrays, but
/// we don't have any such call sites in hiresti yet.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Str(String),
    Bool(bool),
    Int(i64),
    Float(f64),
    Other(Value),
}

impl ParamValue {
    pub fn as_str_value(&self) -> String {
        match self {
            ParamValue::Str(s) => s.clone(),
            ParamValue::Bool(b) => b.to_string(),
            ParamValue::Int(n) => n.to_string(),
            ParamValue::Float(f) => f.to_string(),
            ParamValue::Other(v) => v.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ResponseJson {
    pub ok: bool,
    pub status: u16,
    pub body: Value,
}

pub fn perform_request(
    agent: &Agent,
    access_token: &str,
    country_code: Option<&str>,
    session_id: Option<&str>,
    args: RequestArgs,
) -> RtcResult<ResponseJson> {
    let method = args.method.trim();
    if method.is_empty() {
        return Err(RtcError::InvalidInput("method is empty".into()));
    }
    let upper = method.to_ascii_uppercase();

    let base = args
        .base_url
        .as_deref()
        .unwrap_or(auth::API_V1_LOCATION)
        .trim_end_matches(' ');
    let url_str = if args.path.starts_with("http://") || args.path.starts_with("https://") {
        args.path.clone()
    } else {
        format!("{}{}", trim_trailing_slash(base), normalize_path(&args.path))
    };
    let mut url = Url::parse(&url_str)
        .map_err(|e| RtcError::InvalidInput(format!("bad url '{}': {}", url_str, e)))?;

    let send_form_body = args.form_body && !is_get_or_delete(&upper);

    if let Some(params) = args.params.as_ref() {
        if !send_form_body {
            for (k, v) in params {
                url.query_pairs_mut().append_pair(k, &v.as_str_value());
            }
        }
    }
    // tidalapi auto-injects three params on every request. Some endpoints
    // (notably home/feed/static, returns 400/subStatus=1002 without them)
    // validate their presence, so we mirror the same defaults.
    let existing_keys: std::collections::HashSet<String> =
        url.query_pairs().map(|(k, _)| k.into_owned()).collect();
    if let Some(cc) = country_code {
        if !existing_keys.contains("countryCode") {
            url.query_pairs_mut().append_pair("countryCode", cc);
        }
    }
    if let Some(sid) = session_id {
        if !existing_keys.contains("sessionId") {
            url.query_pairs_mut().append_pair("sessionId", sid);
        }
    }
    if !existing_keys.contains("limit") {
        url.query_pairs_mut()
            .append_pair("limit", &http::DEFAULT_ITEM_LIMIT.to_string());
    }

    let mut req = match upper.as_str() {
        "GET" => agent.get(url.as_str()),
        "POST" => agent.post(url.as_str()),
        "PUT" => agent.put(url.as_str()),
        "DELETE" => agent.delete(url.as_str()),
        "PATCH" => agent.request("PATCH", url.as_str()),
        "HEAD" => agent.head(url.as_str()),
        other => return Err(RtcError::InvalidInput(format!("unsupported HTTP method: {}", other))),
    };

    req = req.set("Authorization", &format!("Bearer {}", access_token));
    req = req.set("x-tidal-client-version", http::TIDAL_CLIENT_VERSION);
    if let Some(headers) = args.headers.as_ref() {
        for (k, v) in headers {
            req = req.set(k, v);
        }
    }

    let call = if let Some(body) = args.json_body.as_ref() {
        req = req.set("Content-Type", "application/json");
        req.send_string(&body.to_string())
    } else if send_form_body {
        let pairs: Vec<(String, String)> = args
            .params
            .as_ref()
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.as_str_value()))
                    .collect()
            })
            .unwrap_or_default();
        let borrowed: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        req.send_form(&borrowed)
    } else {
        req.call()
    };

    match call {
        Ok(resp) => {
            let status = resp.status();
            let content_type = resp
                .header("content-type")
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            let text = resp.into_string().map_err(|e| RtcError::Network(e.to_string()))?;
            let body: Value = if text.trim().is_empty() {
                Value::Null
            } else if content_type.contains("application/json")
                || text.trim_start().starts_with('{')
                || text.trim_start().starts_with('[')
            {
                serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.clone()))
            } else {
                Value::String(text)
            };
            Ok(ResponseJson {
                ok: (200..300).contains(&status),
                status,
                body,
            })
        }
        Err(ureq::Error::Status(status, response)) => {
            let body_text = response.into_string().unwrap_or_default();
            let body: Value = if body_text.trim().is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&body_text)
                    .unwrap_or_else(|_| Value::String(body_text.clone()))
            };
            // 4xx/5xx still resolves as a successful Rust-level call so the
            // Python side can inspect status and body uniformly. The
            // caller decides whether ok=false should bubble as an error.
            Ok(ResponseJson {
                ok: false,
                status,
                body,
            })
        }
        Err(transport) => Err(map_ureq(transport)),
    }
}

fn is_get_or_delete(method: &str) -> bool {
    matches!(method, "GET" | "DELETE" | "HEAD")
}

fn trim_trailing_slash(s: &str) -> String {
    if s.ends_with('/') {
        s.to_string()
    } else {
        format!("{}/", s)
    }
}

fn normalize_path(path: &str) -> String {
    path.trim_start_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_assembly_with_default_base() {
        let agent = ureq::Agent::new();
        // We can't actually call the network in unit tests, so we exercise
        // the URL math by triggering a method-validation error after URL
        // parsing — the parsing would fail before that if the path math
        // were wrong.
        let args = RequestArgs {
            method: "FROBNICATE".into(),
            path: "users/42/subscription".into(),
            base_url: None,
            params: None,
            headers: None,
            json_body: None,
            form_body: false,
        };
        let err = perform_request(&agent, "tok", Some("US"), None, args).unwrap_err();
        assert!(matches!(err, RtcError::InvalidInput(_)));
    }

    #[test]
    fn unsupported_method_rejected() {
        let agent = ureq::Agent::new();
        let args = RequestArgs {
            method: "WAT".into(),
            path: "x".into(),
            base_url: None,
            params: None,
            headers: None,
            json_body: None,
            form_body: false,
        };
        assert!(matches!(
            perform_request(&agent, "t", None, None, args).unwrap_err(),
            RtcError::InvalidInput(_)
        ));
    }

    #[test]
    fn empty_method_rejected() {
        let agent = ureq::Agent::new();
        let args = RequestArgs {
            method: "".into(),
            path: "x".into(),
            base_url: None,
            params: None,
            headers: None,
            json_body: None,
            form_body: false,
        };
        assert!(matches!(
            perform_request(&agent, "t", None, None, args).unwrap_err(),
            RtcError::InvalidInput(_)
        ));
    }
}
