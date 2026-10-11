//! Liaison applicant API over the browser session cookie.

use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{method} {path}: transport: {source}")]
    Transport { method: &'static str, path: String, source: reqwest::Error },
    #[error("{method} {path}: HTTP {status}: {body}")]
    Http { method: &'static str, path: String, status: u16, body: String },
    #[error("{method} {path}: the Liaison session has expired - run `admit session set` with a fresh Cookie header")]
    Expired { method: &'static str, path: String },
    #[error("{method} {path}: response is not JSON: {source}")]
    Json { method: &'static str, path: String, source: serde_json::Error },
    #[error("{0}")]
    Shape(String),
    #[error("sign-in rejected: HTTP {status}: {body}")]
    BadLogin { status: u16, body: String },
    /// admit's own state on this machine (the documents lock) could not be
    /// read or written.
    #[error("{0}")]
    Local(String),
}

pub type Result<T> = std::result::Result<T, ApiError>;

/// A Liaison CAS instance, e.g. `https://nursingcas2027.cas.myliaison.com`.
pub struct Client {
    http: reqwest::Client,
    api: String,
    origin: String,
    cookie: String,
    journal: admit_domain::journal::Journal,
}

impl Client {
    pub fn new(base_url: &str, cookie: String) -> Client {
        let origin = base_url.trim_end_matches('/').to_string();
        Client {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .user_agent(concat!("admit/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_default(),
            api: format!("{origin}/applicant/api"),
            origin,
            cookie,
            journal: Default::default(),
        }
    }

    /// Records every write (not reads) in `journal`, before it is sent and
    /// when it is answered.
    pub fn with_journal(mut self, journal: admit_domain::journal::Journal) -> Client {
        self.journal = journal;
        self
    }

    /// Signs in with the applicant's email and password and returns the
    /// Cookie header the API wants. Captured 2026-10-06: `GET
    /// /applicant/initialize` sets `cas`, `casName`, `formId`; `POST
    /// /applicant/authenticate` with HTTP Basic credentials and body `{}`
    /// answers "Successful Authentication" and sets `SESSION`. No MFA.
    pub async fn login(base_url: &str, user: &str, password: &str) -> Result<String> {
        let origin = base_url.trim_end_matches('/');
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(concat!("admit/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        let transport = |path: &str| {
            let path = path.to_string();
            move |source| ApiError::Transport { method: "POST", path, source }
        };
        let mut jar: Vec<(String, String)> = Vec::new();
        let init = http.get(format!("{origin}/applicant/initialize")).send().await.map_err(transport("/initialize"))?;
        absorb_cookies(&mut jar, init.headers());
        let header =
            |jar: &[(String, String)]| jar.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ");
        let resp = http
            .post(format!("{origin}/applicant/authenticate"))
            .basic_auth(user, Some(password))
            .header("Cookie", header(&jar))
            .header("Accept", "application/json")
            .header("Origin", origin)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(transport("/authenticate"))?;
        let status = resp.status().as_u16();
        absorb_cookies(&mut jar, resp.headers());
        let body = resp.text().await.map_err(transport("/authenticate"))?;
        if !(200..300).contains(&status) || !jar.iter().any(|(k, _)| k == "SESSION") {
            return Err(ApiError::BadLogin { status, body: body.chars().take(300).collect() });
        }
        Ok(header(&jar))
    }

    /// Prefix of every applicant API URL (used to pick the cookie from a HAR).
    pub fn api_prefix(base_url: &str) -> String {
        format!("{}/applicant/api/", base_url.trim_end_matches('/'))
    }

    async fn send(&self, method: &'static str, path: &str, rb: reqwest::RequestBuilder) -> Result<Value> {
        if method == "GET" {
            return self.exchange(method, path, rb).await;
        }
        self.journal.started(method, path).map_err(ApiError::Local)?;
        let answer = self.exchange(method, path, rb).await;
        match &answer {
            Ok(_) => self.journal.done(method, path),
            Err(e) => self.journal.failed(method, path, &e.to_string()),
        }
        .map_err(ApiError::Local)?;
        answer
    }

    async fn exchange(&self, method: &'static str, path: &str, rb: reqwest::RequestBuilder) -> Result<Value> {
        let (_, bytes) = self.raw(method, path, rb, "application/json").await?;
        let text = String::from_utf8_lossy(&bytes);
        if text.trim_start().starts_with('<') {
            return Err(ApiError::Expired { method, path: path.into() });
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|source| ApiError::Json { method, path: path.into(), source })
    }

    /// The response's `Content-Type` and body, after the status checks
    /// every request shares.
    async fn raw(
        &self,
        method: &'static str,
        path: &str,
        rb: reqwest::RequestBuilder,
        accept: &str,
    ) -> Result<(String, Vec<u8>)> {
        let resp = rb
            .header("Cookie", &self.cookie)
            .header("Accept", accept)
            .header("Origin", &self.origin)
            .send()
            .await
            .map_err(|source| ApiError::Transport { method, path: path.into(), source })?;
        let status = resp.status();
        let kind = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let bytes = resp.bytes().await.map_err(|source| ApiError::Transport { method, path: path.into(), source })?;
        // Observed 2026-10: an expired session answers 401 with a JSON
        // "Full authentication is required" body.
        if status.as_u16() == 401 {
            return Err(ApiError::Expired { method, path: path.into() });
        }
        if !status.is_success() {
            return Err(ApiError::Http {
                method,
                path: path.into(),
                status: status.as_u16(),
                body: String::from_utf8_lossy(&bytes).chars().take(400).collect(),
            });
        }
        Ok((kind, bytes.to_vec()))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.api)
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        self.send("GET", path, self.http.get(self.url(path))).await
    }

    /// A file as the portal serves it (a document's `view`). An HTML
    /// answer is the sign-in page: no document the portal takes is HTML.
    pub async fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let (kind, bytes) = self.raw("GET", path, self.http.get(self.url(path)), "*/*").await?;
        if kind.starts_with("text/html") {
            return Err(ApiError::Expired { method: "GET", path: path.into() });
        }
        Ok(bytes)
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.send("POST", path, self.http.post(self.url(path)).json(body)).await
    }

    /// A POST that only reads (the deadline search): not a write, so not
    /// journalled.
    pub async fn query(&self, path: &str, body: &Value) -> Result<Value> {
        self.exchange("POST", path, self.http.post(self.url(path)).json(body)).await
    }

    pub async fn put(&self, path: &str, body: &Value) -> Result<Value> {
        self.send("PUT", path, self.http.put(self.url(path)).json(body)).await
    }

    /// A PUT with no body, as the browser sends a status change (captured:
    /// no body, no `Content-Type`).
    pub async fn put_empty(&self, path: &str) -> Result<Value> {
        self.send("PUT", path, self.http.put(self.url(path))).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        self.send("DELETE", path, self.http.delete(self.url(path))).await
    }

    /// `multipart/form-data` with one JSON part named `model`, the way the
    /// license form saves (create and edit alike; `id` 0 creates).
    pub async fn post_model(&self, path: &str, model: &Value) -> Result<Value> {
        // `mime_str`, not `.headers()`: the part then carries `Content-Type:`
        // as the browser wrote it (captured license save); a header map sends
        // it lowercased. It cannot fail for this literal.
        let part = reqwest::multipart::Part::text(model.to_string())
            .file_name("blob")
            .mime_str("application/json")
            .expect("application/json is a valid MIME type");
        let form = reqwest::multipart::Form::new().part("model", part);
        self.send("POST", path, self.http.post(self.url(path)).multipart(form)).await
    }

    /// `multipart/form-data` with the file, then the JSON `model` part, as
    /// the Documents upload sends them (captured 2026-10-07).
    pub async fn post_file(&self, path: &str, name: &str, mime: &str, bytes: Vec<u8>, model: &Value) -> Result<Value> {
        let shape = |e: reqwest::Error| ApiError::Shape(format!("{path}: {e}"));
        let file = reqwest::multipart::Part::bytes(bytes).file_name(name.to_string()).mime_str(mime).map_err(shape)?;
        let model = reqwest::multipart::Part::text(model.to_string())
            .file_name("blob")
            .mime_str("application/json")
            .expect("application/json is a valid MIME type");
        let form = reqwest::multipart::Form::new().part("file", file).part("model", model);
        self.send("POST", path, self.http.post(self.url(path)).multipart(form)).await
    }
}

/// Applies `Set-Cookie` headers to a name-ordered jar (a cleared cookie,
/// `Max-Age=0`, is removed).
pub(crate) fn absorb_cookies(jar: &mut Vec<(String, String)>, headers: &reqwest::header::HeaderMap) {
    for v in headers.get_all(reqwest::header::SET_COOKIE).iter().filter_map(|v| v.to_str().ok()) {
        let mut parts = v.split(';').map(str::trim);
        let Some((name, value)) = parts.next().and_then(|nv| nv.split_once('=')) else { continue };
        let cleared = parts.any(|a| a.eq_ignore_ascii_case("max-age=0"));
        jar.retain(|(k, _)| k != name);
        if !cleared {
            jar.push((name.to_string(), value.to_string()));
        }
    }
}

/// Field accessors that turn API JSON into plain values.
pub(crate) fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub(crate) fn opt(v: &Value) -> Option<String> {
    Some(s(v)).filter(|x| !x.trim().is_empty() && x != "None")
}

pub(crate) fn int(v: &Value) -> i64 {
    v.as_i64().or_else(|| v.as_str().and_then(|x| x.parse().ok())).unwrap_or(0)
}

pub(crate) fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}

#[cfg(test)]
#[path = "tests/client_scalar_tests.rs"]
mod scalar_tests;
