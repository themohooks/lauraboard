//! Tiny blocking HTTP client for the local API (`ctl` commands).

use std::time::Duration;

use anyhow::Result;
use serde_json::Value;

use lb_core::config::Role;
use lb_core::project::Project;

use crate::exit::{Code, bail};

pub struct Client {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl Client {
    /// Client for the project's configured API address.
    pub fn for_project(p: &Project, token: Option<&str>) -> Self {
        let host = p.app.api.bind.replace("0.0.0.0", "127.0.0.1").replace("[::]", "[::1]");
        let token = token
            .map(str::to_string)
            .or_else(|| p.app.api.tokens.iter().max_by_key(|t| t.role).filter(|t| t.role >= Role::Operator).map(|t| t.token.clone()));
        Self::new(&format!("http://{host}"), token, Duration::from_secs(30))
    }

    pub fn new(base: &str, token: Option<String>, timeout: Duration) -> Self {
        let cfg = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(timeout)).build();
        Client { base: format!("{}/api/v1", base.trim_end_matches('/')), token, agent: ureq::Agent::new_with_config(cfg) }
    }

    fn handle(&self, res: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<String> {
        let mut res =
            res.map_err(|e| bail(Code::Connection, format!("cannot reach the API at {}: {e} (is `lauraboard run` running?)", self.base)))?;
        let status = res.status().as_u16();
        let body = res.body_mut().read_to_string().unwrap_or_default();
        if status >= 400 {
            let msg = serde_json::from_str::<Value>(&body).ok().and_then(|v| v["error"].as_str().map(str::to_string)).unwrap_or(body);
            let code = match status {
                404 => Code::NotFound,
                400 => Code::Invalid,
                409 => Code::AlreadyRunning,
                401 | 403 => Code::Connection,
                _ => Code::Error,
            };
            return Err(bail(code, format!("API error {status}: {msg}")));
        }
        Ok(body)
    }

    pub fn get(&self, path: &str) -> Result<String> {
        let mut r = self.agent.get(format!("{}{path}", self.base));
        if let Some(t) = &self.token {
            r = r.header("Authorization", format!("Bearer {t}"));
        }
        self.handle(r.call())
    }

    pub fn get_json(&self, path: &str) -> Result<Value> {
        Ok(serde_json::from_str(&self.get(path)?)?)
    }

    pub fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        let mut r = self.agent.post(format!("{}{path}", self.base)).header("content-type", "application/json");
        if let Some(t) = &self.token {
            r = r.header("Authorization", format!("Bearer {t}"));
        }
        let text = self.handle(r.send(body.to_string().as_str()))?;
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
}

/// After offline edits, ask a running instance to reload (silently).
pub fn notify_reload(p: &Project, channel: &str, token: Option<&str>) {
    if !p.app.api.enabled {
        return;
    }
    let mut c = Client::for_project(p, token);
    c.agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(2))).build(),
    );
    if c.post_json(&format!("/channels/{channel}/command"), &serde_json::json!({ "cmd": "reload" })).is_ok() {
        eprintln!("running instance reloaded");
    }
}
