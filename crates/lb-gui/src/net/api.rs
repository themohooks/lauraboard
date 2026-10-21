//! Blocking HTTP client for the Laurabrod API.

use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

#[derive(Clone)]
pub struct Api {
    pub base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl Api {
    pub fn new(base: &str, token: Option<String>) -> Self {
        let cfg = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(20))).build();
        Api { base: base.trim_end_matches('/').to_string(), token, agent: ureq::Agent::new_with_config(cfg) }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base)
    }

    /// WebSocket URL for the event stream (token in the query string).
    pub fn ws_url(&self, path: &str) -> String {
        let mut u = self.url(path).replacen("http://", "ws://", 1).replacen("https://", "wss://", 1);
        if let Some(t) = &self.token {
            u.push_str(&format!("?token={t}"));
        }
        u
    }

    fn finish(res: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<Vec<u8>, String> {
        let mut res = res.map_err(|e| format!("connection failed: {e}"))?;
        let status = res.status().as_u16();
        let body = res.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
        if status >= 400 {
            let text = String::from_utf8_lossy(&body).to_string();
            let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"].as_str().map(str::to_string)).unwrap_or(text);
            return Err(format!("{status}: {msg}"));
        }
        Ok(body)
    }

    pub fn get_bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        let mut r = self.agent.get(self.url(path));
        if let Some(t) = &self.token {
            r = r.header("Authorization", format!("Bearer {t}"));
        }
        Self::finish(r.call())
    }

    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        serde_json::from_slice(&self.get_bytes(path)?).map_err(|e| format!("bad response from {path}: {e}"))
    }

    fn send<B: Serialize>(&self, method: &str, path: &str, body: &B) -> Result<Value, String> {
        let json = serde_json::to_string(body).map_err(|e| e.to_string())?;
        let url = self.url(path);
        let mut r = match method {
            "PUT" => self.agent.put(url),
            _ => self.agent.post(url),
        }
        .header("content-type", "application/json");
        if let Some(t) = &self.token {
            r = r.header("Authorization", format!("Bearer {t}"));
        }
        let bytes = Self::finish(r.send(json.as_str()))?;
        Ok(serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    pub fn post<B: Serialize>(&self, path: &str, body: &B) -> Result<Value, String> {
        self.send("POST", path, body)
    }

    pub fn put<B: Serialize>(&self, path: &str, body: &B) -> Result<Value, String> {
        self.send("PUT", path, body)
    }

    pub fn delete(&self, path: &str) -> Result<Value, String> {
        let mut r = self.agent.delete(self.url(path));
        if let Some(t) = &self.token {
            r = r.header("Authorization", format!("Bearer {t}"));
        }
        let bytes = Self::finish(r.call())?;
        Ok(serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }
}
