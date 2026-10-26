//! Blocking HTTP client for the LauraBoard API.

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

    /// The same client with a longer timeout, for requests where the
    /// server itself waits on the internet (font downloads).
    pub fn patient(&self, secs: u64) -> Api {
        let cfg = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(secs))).build();
        Api { base: self.base.clone(), token: self.token.clone(), agent: ureq::Agent::new_with_config(cfg) }
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
        let mut res = res.map_err(|e| format!("Нет связи с сервером: {e}"))?;
        let status = res.status().as_u16();
        let body = res.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
        if status >= 400 {
            let text = String::from_utf8_lossy(&body).to_string();
            let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"].as_str().map(str::to_string)).unwrap_or(text);
            return Err(humanize(status, &msg));
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

    /// POST returning the raw body (images).
    pub fn post_bytes<B: Serialize>(&self, path: &str, body: &B) -> Result<Vec<u8>, String> {
        let json = serde_json::to_string(body).map_err(|e| e.to_string())?;
        let mut r = self.agent.post(self.url(path)).header("content-type", "application/json");
        if let Some(t) = &self.token {
            r = r.header("Authorization", format!("Bearer {t}"));
        }
        Self::finish(r.send(json.as_str()))
    }

    /// PUT of raw bytes (pictures).
    pub fn put_bytes(&self, path: &str, content_type: &str, bytes: &[u8]) -> Result<Value, String> {
        let mut r = self.agent.put(self.url(path)).header("content-type", content_type);
        if let Some(t) = &self.token {
            r = r.header("Authorization", format!("Bearer {t}"));
        }
        let out = Self::finish(r.send(bytes))?;
        Ok(serde_json::from_slice(&out).unwrap_or(Value::Null))
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

/// Server error → text for the operator: what failed and every reason,
/// instead of a bare status code.
pub fn humanize(status: u16, msg: &str) -> String {
    let msg = msg.trim();
    if let Some(list) = msg.strip_prefix("validation failed:") {
        let items: Vec<String> = list.split("; ").map(|x| format!("• {}", x.trim())).filter(|x| x.len() > 4).collect();
        return format!(
            "Не сохранено — проверка канала нашла ошибки:
{}",
            items.join(
                "
"
            )
        );
    }
    // channel start (engine pre-flight check)
    if let Some(m) = msg.strip_prefix("validation failed with") {
        return format!(
            "Канал не прошёл проверку перед запуском —{}. Подробности: «Журнал» или `lauraboard validate`",
            m.replace("error(s); first:", "ошибок, первая:")
        );
    }
    if let Some(m) = msg.strip_prefix("invalid:") {
        return format!("Неверные данные: {}", m.trim());
    }
    // axum's JSON extractor (shape of the document did not match)
    if let Some((_, m)) = msg.split_once("into the target type:") {
        return format!("Сервер не принял данные: {}", m.trim());
    }
    let ru = crate::ru::server(msg);
    // a known message says enough without the status code
    if ru != msg {
        return ru;
    }
    match status {
        401 => "Нет доступа: неверный или отсутствующий токен API".into(),
        403 => format!("Недостаточно прав: {msg}"),
        404 => format!("Не найдено: {msg}"),
        409 => format!("Конфликт: {msg}"),
        _ => format!("Ошибка {status}: {msg}"),
    }
}

#[cfg(test)]
mod tests {
    use super::humanize;

    #[test]
    fn validation_errors_are_listed() {
        let t = humanize(400, "validation failed: graphics/rules/a: unknown element 'x'; schedule: bad");
        assert!(t.starts_with("Не сохранено"));
        assert!(
            t.contains(
                "
• graphics/rules/a: unknown element 'x'
• schedule: bad"
            ),
            "{t}"
        );
        assert_eq!(
            humanize(422, "Failed to deserialize the JSON body into the target type: missing field `id`"),
            "Сервер не принял данные: missing field `id`"
        );
    }
}
