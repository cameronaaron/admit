//! A Slate application portal (`https://<host>/apply/…`) over the browser
//! session cookie.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{method} {path}: transport: {source}")]
    Transport { method: &'static str, path: String, source: reqwest::Error },
    #[error("{method} {path}: HTTP {status}: {body}")]
    Http { method: &'static str, path: String, status: u16, body: String },
    #[error(
        "{path}: the session has expired (redirected to sign-in) - copy the Cookie header of any /apply/ request and run `pbpaste | admit -p <portal> session set`"
    )]
    Expired { path: String },
    #[error("{0}")]
    Shape(String),
    /// `message` is what the page said (`<p class="error">`), empty if nothing.
    #[error(
        "sign-in rejected (HTTP {status}){}: check the email and password stored with `admit -p <portal> session login`",
        if message.is_empty() { String::new() } else { format!(": \"{message}\"") }
    )]
    BadLogin { status: u16, message: String },
    /// The write journal could not be written, so the write was not sent.
    #[error("{0}")]
    Local(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Client {
    journal: admit_domain::journal::Journal,
    http: reqwest::Client,
    origin: String,
    apply: String,
    cookie: String,
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("admit/", env!("CARGO_PKG_VERSION")))
        // A redirect to /account/login is how Slate says "expired";
        // following it would hide that.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
}

impl Client {
    /// `base_url` is the portal's origin, e.g. `https://apply.grad.ucla.edu`.
    pub fn new(base_url: &str, cookie: String) -> Client {
        let origin = base_url.trim_end_matches('/').to_string();
        Client { journal: Default::default(), http: http(), apply: format!("{origin}/apply/"), origin, cookie }
    }

    /// Records every save in `journal`, before it is sent and when it is
    /// answered.
    pub fn with_journal(mut self, journal: admit_domain::journal::Journal) -> Client {
        self.journal = journal;
        self
    }

    /// Runs one save between its journal lines.
    async fn journalled(&self, path: &str, save: impl Future<Output = Result<String>>) -> Result<String> {
        self.journal.started("POST", path).map_err(Error::Local)?;
        let answer = save.await;
        match &answer {
            Ok(_) => self.journal.done("POST", path),
            Err(e) => self.journal.failed("POST", path, &e.to_string()),
        }
        .map_err(Error::Local)?;
        answer
    }

    /// The Cookie header in use (after [`Client::open`], it names the
    /// application).
    pub fn cookie(&self) -> &str {
        &self.cookie
    }

    /// Signs in and returns the session's Cookie header. Captured
    /// 2026-10-06: `GET /account/login` answers 307 to `…&cookie=1`, setting
    /// `_cookie_test`, `_hash`, `_hashV`, `_node` (a client that doesn't send
    /// them back gets 503); the form then POSTs `email` and `password`
    /// (urlencoded), and success is a 302 to `r` that sets `_uid`. Every
    /// POST must carry `Origin` (as browsers send it): without it Slate
    /// answers sign-in with a 302 but no `_uid`, and any other POST with a
    /// redirect to sign-in, which reads like an expired session (observed
    /// 2026-10-06).
    pub async fn login(base_url: &str, email: &str, password: &str) -> Result<String> {
        let origin = base_url.trim_end_matches('/');
        let http = http();
        let mut jar = String::new();
        let login = format!("{origin}/account/login?r={}", urlencode(&format!("{origin}/apply/")));
        let mut url = login.clone();
        let transport = |source| Error::Transport { method: "GET", path: "/account/login".into(), source };
        for _ in 0..3 {
            let resp = http.get(&url).header("Cookie", &jar).send().await.map_err(transport)?;
            absorb(&mut jar, resp.headers());
            match resp.headers().get("location").and_then(|v| v.to_str().ok()) {
                Some(next) if resp.status().is_redirection() => {
                    url = if next.starts_with('/') { format!("{origin}{next}") } else { next.to_string() };
                }
                _ => break,
            }
        }
        // The browser posts to the login URL itself, not the cookie-test one.
        let resp = http
            .post(&login)
            .header("Cookie", &jar)
            .header("Origin", origin)
            .form(&[("email", email), ("password", password)])
            .send()
            .await
            .map_err(|source| Error::Transport { method: "POST", path: "/account/login".into(), source })?;
        let status = resp.status();
        absorb(&mut jar, resp.headers());
        // Headers can arrive before a body that then cuts off. A short body
        // is a transport failure, the same as any other response.
        let body = resp.bytes().await.map_err(|source| Error::Transport {
            method: "POST",
            path: "/account/login".into(),
            source,
        })?;
        if !status.is_redirection() || !has(&jar, "_uid") {
            let message = crate::page::errors(&String::from_utf8_lossy(&body)).join(" ");
            return Err(Error::BadLogin { status: status.as_u16(), message });
        }
        Ok(jar)
    }

    /// Makes `id` the application every `/apply/` page reads and writes.
    /// Slate keeps the open application in the `.application` cookie; this
    /// is the "Open Application" button of the `/apply/` list (captured
    /// 2026-10-06: POST `?id=<id>&cmd=detail`, body `cmd=save`, answered
    /// with `Set-Cookie: .application=<id>`).
    pub async fn open(&mut self, id: &str) -> Result<()> {
        let path = format!("?id={id}&cmd=detail");
        let resp = self
            .http
            .post(format!("{}{path}", self.apply))
            .header("Cookie", &self.cookie)
            .header("Origin", &self.origin)
            .form(&[("cmd", "save")])
            .send()
            .await
            .map_err(|source| Error::Transport { method: "POST", path: path.clone(), source })?;
        let mut jar = self.cookie.clone();
        absorb(&mut jar, resp.headers());
        self.body("POST", &path, resp).await?;
        if cookie_value(&jar, ".application") != Some(id) {
            return Err(Error::Shape(format!(
                "opening application {id}: the portal did not switch to it (no .application cookie for it)"
            )));
        }
        self.cookie = jar;
        Ok(())
    }

    /// GET `/apply/<path>` (e.g. `frm?<guid>`, `review`).
    pub async fn get(&self, path: &str) -> Result<String> {
        let resp = self
            .http
            .get(format!("{}{path}", self.apply))
            .header("Cookie", &self.cookie)
            .send()
            .await
            .map_err(|source| Error::Transport { method: "GET", path: path.into(), source })?;
        self.body("GET", path, resp).await
    }

    /// The page's autocomplete service (`Suggest`): `GET
    /// /manage/service/lookup?type=<kind>&q=<query>` (captured 2026-10-06 in
    /// the Survey page's script), answered with the matches the dropdown
    /// shows; picking one sets the visible name and a hidden code field.
    pub async fn lookup(&self, kind: &str, query: &str) -> Result<String> {
        let origin = self.apply.trim_end_matches("/apply/");
        let path = format!("/manage/service/lookup?type={}&q={}", urlencode(kind), urlencode(query));
        let resp = self
            .http
            .get(format!("{origin}{path}"))
            .header("Cookie", &self.cookie)
            .send()
            .await
            .map_err(|source| Error::Transport { method: "GET", path: path.clone(), source })?;
        self.body("GET", &path, resp).await
    }

    /// Add Institution's school lookup (`aca.js`, fetched 2026-10-06): a
    /// region with more than 25 schools lists cities first,
    /// `aca?cmd=fetch_cities&region=US:CA` (JSON array of names); schools
    /// come from `aca?cmd=fetch_schools&region=…[&city=…]` as
    /// `[key, name, city, region]` rows.
    pub async fn fetch_cities(&self, region: &str) -> Result<String> {
        self.get(&format!("aca?cmd=fetch_cities&region={}", urlencode(region))).await
    }

    pub async fn fetch_schools(&self, region: &str, city: Option<&str>) -> Result<String> {
        let city = city.map(|c| format!("&city={}", urlencode(c))).unwrap_or_default();
        self.get(&format!("aca?cmd=fetch_schools&region={}{city}", urlencode(region))).await
    }

    /// GET `/apply/<path>` as bytes (a PDF stream).
    pub async fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let resp = self
            .http
            .get(format!("{}{path}", self.apply))
            .header("Cookie", &self.cookie)
            .send()
            .await
            .map_err(|source| Error::Transport { method: "GET", path: path.into(), source })?;
        let status = resp.status().as_u16();
        if (300..400).contains(&status) {
            let to = resp.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default();
            if to.contains("/account/login") {
                return Err(Error::Expired { path: path.into() });
            }
        }
        if !(200..300).contains(&status) {
            return Err(Error::Http { method: "GET", path: path.into(), status, body: String::new() });
        }
        Ok(resp.bytes().await.map_err(|source| Error::Transport { method: "GET", path: path.into(), source })?.to_vec())
    }

    /// Saves a page the way its Continue button does: a multipart POST of
    /// `dirty=1` and the fields. Slate clears the page's fields that are not
    /// sent, so `fields` must be the whole page ([`crate::answer::page_post`]).
    pub async fn save(&self, path: &str, fields: &[(String, String)], empty_files: &[String]) -> Result<String> {
        let mut form = reqwest::multipart::Form::new().text("dirty", "1");
        for (k, v) in fields {
            form = form.text(k.clone(), v.clone());
        }
        for name in empty_files {
            // What a browser sends for a file input with nothing chosen.
            // `mime_str`, not `.headers()`: a header map sends the part's
            // `Content-Type:` lowercased (caught once by the wire test).
            let part = reqwest::multipart::Part::bytes(Vec::new())
                .file_name("")
                .mime_str("application/octet-stream")
                .expect("application/octet-stream is a valid MIME type");
            form = form.part(name.clone(), part);
        }
        self.journalled(path, async {
            let resp = self
                .http
                .post(format!("{}{path}", self.apply))
                .header("Cookie", &self.cookie)
                .header("Origin", &self.origin)
                .multipart(form)
                .send()
                .await
                .map_err(|source| Error::Transport { method: "POST", path: path.into(), source })?;
            self.body("POST", path, resp).await
        })
        .await
    }

    /// A urlencoded form post (a popup form without an enctype, e.g. Add
    /// Employer).
    pub async fn save_form(&self, path: &str, fields: &[(String, String)]) -> Result<String> {
        self.journalled(path, async {
            let resp = self
                .http
                .post(format!("{}{path}", self.apply))
                .header("Cookie", &self.cookie)
                .header("Origin", &self.origin)
                .form(fields)
                .send()
                .await
                .map_err(|source| Error::Transport { method: "POST", path: path.into(), source })?;
            self.body("POST", path, resp).await
        })
        .await
    }

    /// A multipart save with one file attached (Add Institution's
    /// transcript, `file1`).
    pub async fn save_file(
        &self,
        path: &str,
        fields: &[(String, String)],
        file_field: &str,
        file_name: &str,
        bytes: Vec<u8>,
    ) -> Result<String> {
        let mut form = reqwest::multipart::Form::new().text("dirty", "1");
        for (k, v) in fields {
            form = form.text(k.clone(), v.clone());
        }
        // `mime_str`, as in `save`: the `file1` wire test pins the header.
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(file_name.to_string())
            .mime_str("application/pdf")
            .expect("application/pdf is a valid MIME type");
        form = form.part(file_field.to_string(), part);
        self.journalled(path, async {
            let resp = self
                .http
                .post(format!("{}{path}", self.apply))
                .header("Cookie", &self.cookie)
                .header("Origin", &self.origin)
                .multipart(form)
                .send()
                .await
                .map_err(|source| Error::Transport { method: "POST", path: path.into(), source })?;
            self.body("POST", path, resp).await
        })
        .await
    }

    async fn body(&self, method: &'static str, path: &str, resp: reqwest::Response) -> Result<String> {
        let status = resp.status().as_u16();
        if (300..400).contains(&status) {
            let to = resp.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default();
            if to.contains("/account/login") {
                return Err(Error::Expired { path: path.into() });
            }
            return Err(Error::Http { method, path: path.into(), status, body: format!("redirect to {to}") });
        }
        let body = resp.text().await.map_err(|source| Error::Transport { method, path: path.into(), source })?;
        if !(200..300).contains(&status) {
            return Err(Error::Http { method, path: path.into(), status, body: body.chars().take(300).collect() });
        }
        Ok(body)
    }
}

/// Folds `Set-Cookie` headers into a Cookie header, replacing same-named
/// cookies.
fn absorb(jar: &mut String, headers: &reqwest::header::HeaderMap) {
    for v in headers.get_all("set-cookie").iter().filter_map(|v| v.to_str().ok()) {
        let pair = v.split(';').next().unwrap_or_default().trim();
        if let Some((name, _)) = pair.split_once('=') {
            let kept: Vec<&str> =
                jar.split("; ").filter(|c| !c.is_empty() && c.split_once('=').map(|(n, _)| n) != Some(name)).collect();
            *jar = kept.into_iter().chain([pair]).collect::<Vec<_>>().join("; ");
        }
    }
}

fn cookie_value<'a>(jar: &'a str, name: &str) -> Option<&'a str> {
    jar.split("; ").find_map(|c| c.split_once('=').filter(|(n, _)| *n == name).map(|(_, v)| v))
}

fn has(jar: &str, name: &str) -> bool {
    cookie_value(jar, name).is_some_and(|v| !v.is_empty())
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02x}"),
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/client_tests.rs"]
mod tests;
