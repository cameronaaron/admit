//! Parchment (`www.parchment.com/lp`, the learner portal): sign in and read
//! the account's transcript orders.
//!
//! Captured 2026-10-10 (`captures/parchment/`); the order read was run live
//! the same day on the browser's session.
//!
//! Orders: `GET /lp/orders` is a Next.js page. The order list is not
//! fetched by the browser from an API: it is in the page's flight data,
//! the strings pushed by `self.__next_f.push([1, "…"])` scripts, which
//! joined hold a line `…{"orders":[…],"onLoadMore":…}`. Each order has
//! `orderLineItems`, one per transcript sent, with `deliveryId` (`TR…`),
//! `orderLineItemStatus` (`COMPLETE` on all five seen, all electronic),
//! `holdType` (`NONE`), `cancelType` (null), the sender
//! (`student.sendingMemberName`) and `recipient.address.destinationName`.
//! Only `COMPLETE` is known to mean sent; a hold type other than `NONE` or
//! any cancel type is read as held; anything else as still open. The page
//! offers more orders through `onLoadMore` (a server action, never
//! captured): only the first page is read, and nothing says how long one
//! is (it held all five).
//!
//! Signed out, `/lp/orders` redirects to `/lp/signIn`.
//!
//! Sign-in is next-auth in front of Keycloak (`auth.parchment.com`, realm
//! `parchment`):
//!
//! 1. `GET /lp/api/auth/csrf` → `{"csrfToken"}` (and its cookie).
//! 2. `POST /lp/api/auth/signin/keycloak`, form `callbackUrl`,
//!    `csrfToken`, `json=true` → `{"url"}`: Keycloak's authorize URL.
//! 3. `GET` it. With a live Keycloak session it redirects straight back
//!    (`/lp/api/auth/callback/keycloak` sets
//!    `__Secure-next-auth.session-token`); otherwise it is the sign-in
//!    page. It has no `<form>` (fetched signed out, 2026-10-10): its
//!    `bundle/login.js` draws one into `<div data-react-component="LoginForm"
//!    data-input-error="…">` from the `commonProps` script, whose
//!    `url.loginAction` is where it posts. Posted as the browser did:
//!    `username`, `password`, `credentialId=`, `login=`. The first version
//!    looked for a `<form>` with a `username` field, from a page that was
//!    never seen, and failed on the live one.
//! 4. Keycloak then emails a one-time code to the account's address
//!    ("A verification code has been sent to your email"): form
//!    `kc-otp-login-form`, posted as `otp=<code>&submit=Submit` (the form
//!    also has a Resend button, never sent).
//! 5. "Trust this device?": form `kc-form-trusted-device`, posted as
//!    `trusted-device-name=<its hidden value>&trusted-device=yes`. That sets
//!    `KEYCLOAK_TRUSTED_DEVICE` with `Max-Age=86400`: for a day, step 4 is
//!    skipped. So an unattended sign-in works at most a day after a
//!    person last typed a code; after that it needs one again.
//!
//! The portal session (`/lp/api/auth/session` `expires`) was about two
//! hours ahead and moved forward as the site was used.

use std::sync::Arc;

use admit_domain::orders::{Order, Stage};
use reqwest::Url;
use reqwest::cookie::{CookieStore, Jar};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Parchment {step}: transport: {source}")]
    Transport { step: &'static str, source: reqwest::Error },
    #[error("Parchment {step}: HTTP {status}")]
    Http { step: &'static str, status: u16 },
    #[error("Parchment session expired or missing (sign in: admit parchment login)")]
    Expired,
    #[error("Parchment sign-in rejected: {0}")]
    Rejected(String),
    #[error("Parchment sent a sign-in code by email; it is needed to sign in (admit parchment login)")]
    CodeNeeded,
    #[error("Parchment {step}: {what} (the site changed?)")]
    Shape { step: &'static str, what: &'static str },
    #[error("Parchment {step}: bad URL {url:?}")]
    Url { step: &'static str, url: String },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub www: String,
    pub auth: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints { www: "https://www.parchment.com".into(), auth: "https://auth.parchment.com".into() }
    }
}

/// Cookies worth keeping between runs, as Cookie headers: the portal
/// session, and Keycloak's (its sign-in session and the trusted device).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Saved {
    pub session: Option<String>,
    pub device: Option<String>,
}

/// A browser user agent, as the captured requests had.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

pub struct Client {
    http: reqwest::Client,
    jar: Arc<Jar>,
    portal: Url,
    keycloak: Url,
    csrf: Url,
    sign_in: Url,
}

struct Page {
    url: Url,
    body: String,
}

fn url(step: &'static str, s: &str) -> Result<Url> {
    Url::parse(s).map_err(|_| Error::Url { step, url: s.into() })
}

impl Client {
    pub fn new(ep: &Endpoints, saved: &Saved) -> Result<Client> {
        let www = ep.www.trim_end_matches('/');
        let auth = ep.auth.trim_end_matches('/');
        // A cookie added for a URL gets that URL's directory as its path:
        // next-auth's are on /lp, Keycloak's on /auth/realms/parchment.
        let portal = url("endpoints", &format!("{www}/lp/orders"))?;
        let keycloak = url("endpoints", &format!("{auth}/auth/realms/parchment/"))?;
        let at = |p: &str| {
            let mut u = portal.clone();
            u.set_path(p);
            u
        };
        let (csrf, sign_in) = (at("/lp/api/auth/csrf"), at("/lp/api/auth/signin/keycloak"));
        let jar = Arc::new(Jar::default());
        for (header, at) in [(&saved.session, &portal), (&saved.device, &keycloak)] {
            for c in header.iter().flat_map(|h| h.split(';')).map(str::trim).filter(|c| c.contains('=')) {
                jar.add_cookie_str(c, at);
            }
        }
        let http = reqwest::Client::builder()
            .cookie_provider(jar.clone())
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_default();
        Ok(Client { http, jar, portal, keycloak, csrf, sign_in })
    }

    /// The cookies to keep for next time.
    pub fn saved(&self) -> Saved {
        let header = |u: &Url| self.jar.cookies(u).and_then(|v| v.to_str().ok().map(str::to_string));
        Saved { session: header(&self.portal), device: header(&self.keycloak) }
    }

    async fn page(&self, step: &'static str, rb: reqwest::RequestBuilder) -> Result<Page> {
        let resp = rb.send().await.map_err(|source| Error::Transport { step, source })?;
        if !resp.status().is_success() {
            return Err(Error::Http { step, status: resp.status().as_u16() });
        }
        let url = resp.url().clone();
        let body = resp.text().await.map_err(|source| Error::Transport { step, source })?;
        Ok(Page { url, body })
    }

    fn on_portal(&self, u: &Url) -> bool {
        u.host_str() == self.portal.host_str() && u.port_or_known_default() == self.portal.port_or_known_default()
    }

    /// The account's orders, one per transcript sent.
    pub async fn orders(&self) -> Result<Vec<Order>> {
        let page = self.page("orders", self.http.get(self.portal.clone())).await?;
        if !self.on_portal(&page.url) || page.url.path() != self.portal.path() {
            return Err(Error::Expired);
        }
        orders(&page.body)
    }

    /// Signs in; `code` is asked for the emailed one-time code when Keycloak
    /// wants one (`None`: there is nobody to ask).
    pub async fn sign_in(&self, user: &str, password: &str, code: &mut dyn FnMut() -> Option<String>) -> Result<()> {
        let csrf: Value = serde_json::from_str(&self.page("csrf", self.http.get(self.csrf.clone())).await?.body)
            .map_err(|_| Error::Shape { step: "csrf", what: "not JSON" })?;
        let token = csrf["csrfToken"].as_str().ok_or(Error::Shape { step: "csrf", what: "no csrfToken" })?;
        let start = self
            .page(
                "sign-in start",
                self.http.post(self.sign_in.clone()).form(&[
                    ("callbackUrl", "/lp/orders"),
                    ("csrfToken", token),
                    ("json", "true"),
                ]),
            )
            .await?;
        let start: Value =
            serde_json::from_str(&start.body).map_err(|_| Error::Shape { step: "sign-in start", what: "not JSON" })?;
        let to = start["url"].as_str().ok_or(Error::Shape { step: "sign-in start", what: "no url" })?;
        let mut page = self.page("Keycloak", self.http.get(url("Keycloak", to)?)).await?;
        let (mut sent_password, mut sent_code) = (false, false);
        // Each Keycloak page is one form: password, code, trust; a form
        // shown again means Keycloak refused what was posted to it.
        for _ in 0..4 {
            if self.on_portal(&page.url) {
                return Ok(());
            }
            let forms = admit_html::forms(&page.body);
            let (action, fields): (String, Vec<(&str, String)>) = if let Some(LoginForm { action, error }) =
                login_form(&page.body)
            {
                if sent_password {
                    let what = if error.is_empty() { "username or password not accepted".into() } else { error };
                    return Err(Error::Rejected(what));
                }
                sent_password = true;
                let fields = vec![
                    ("username", user.to_string()),
                    ("password", password.to_string()),
                    ("credentialId", String::new()),
                    ("login", String::new()),
                ];
                (action, fields)
            } else if let Some(f) = forms.iter().find(|f| f.has("otp")) {
                if sent_code {
                    let what = feedback(&page.body).unwrap_or_else(|| "the code was not accepted".into());
                    return Err(Error::Rejected(what));
                }
                sent_code = true;
                let c = code().ok_or(Error::CodeNeeded)?;
                (f.action.clone(), vec![("otp", c.trim().to_string()), ("submit", "Submit".into())])
            } else if let Some(f) = forms.iter().find(|f| f.has("trusted-device-name")) {
                let name = f.fields.iter().find(|(n, _)| n == "trusted-device-name").map(|(_, v)| v.clone());
                let fields = vec![("trusted-device-name", name.unwrap_or_default()), ("trusted-device", "yes".into())];
                (f.action.clone(), fields)
            } else {
                return Err(match feedback(&page.body) {
                    Some(m) => Error::Rejected(m),
                    None => Error::Shape { step: "Keycloak", what: "a page with no sign-in, code or trust form" },
                });
            };
            let action = page.url.join(&action).map_err(|_| Error::Url { step: "Keycloak", url: action.clone() })?;
            page = self.page("Keycloak", self.http.post(action).form(&fields)).await?;
        }
        Err(Error::Shape { step: "Keycloak", what: "sign-in did not finish after four pages" })
    }
}

/// The sign-in page's React form: where it posts (`commonProps`'
/// `url.loginAction`, a JSON string) and the message it shows
/// (`data-input-error`, empty on a first visit).
#[derive(Debug, PartialEq, Eq)]
struct LoginForm {
    action: String,
    error: String,
}

fn login_form(html: &str) -> Option<LoginForm> {
    const KEY: &str = "\"loginAction\":";
    let at = html.find("data-react-component=\"LoginForm\"")?;
    let open = html[..at].rfind('<')?;
    let tag = &html[open..=admit_html::tag_end(html, open)?];
    let error = admit_html::attr(tag, "data-input-error").unwrap_or_default().trim().to_string();
    let rest = html[html.find(KEY)? + KEY.len()..].trim_start();
    let action = serde_json::Deserializer::from_str(rest).into_iter::<String>().next()?.ok()?;
    Some(LoginForm { action, error })
}

/// `html` without its `<script>` elements: their text is data (the theme's
/// class names, `"pf-c-alert__title kc-feedback-text"` among them), never
/// a message. Reading them printed a whole script as the refusal.
fn visible(html: &str) -> String {
    const CLOSE: &str = "</script>";
    let lower = html.to_ascii_lowercase();
    let (mut out, mut at) = (String::new(), 0);
    while let Some(s) = lower[at..].find("<script").map(|i| i + at) {
        out.push_str(&html[at..s]);
        at = lower[s..].find(CLOSE).map_or(html.len(), |e| s + e + CLOSE.len());
    }
    out + &html[at..]
}

/// Keycloak's message on a refused form (`input-error…` or the alert).
fn feedback(html: &str) -> Option<String> {
    let html = &visible(html);
    for marker in ["id=\"input-error", "pf-c-alert__title", "kc-feedback-text"] {
        let Some(at) = html.find(marker) else { continue };
        let Some(open) = html[..at].rfind('<') else { continue };
        let Some(open_end) = admit_html::tag_end(html, open) else { continue };
        let rest = &html[open_end + 1..];
        let text = admit_html::unescape(rest[..rest.find('<').unwrap_or(rest.len())].trim());
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

/// The flight data of a Next.js page: every `self.__next_f.push([1, s])`
/// string, joined.
pub fn flight(html: &str) -> String {
    const PUSH: &str = "self.__next_f.push(";
    let mut out = String::new();
    let mut at = 0;
    while let Some(i) = html[at..].find(PUSH) {
        let rest = &html[at + i + PUSH.len()..];
        at += i + PUSH.len();
        let mut values = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
        if let Some(Ok(Value::Array(a))) = values.next()
            && let [Value::Number(n), Value::String(s)] = a.as_slice()
            && n.as_u64() == Some(1)
        {
            out.push_str(s);
        }
    }
    out
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().trim().to_string()
}

/// The order list on the orders page.
pub fn orders(html: &str) -> Result<Vec<Order>> {
    let data = flight(html);
    let at = data.find("{\"orders\":").ok_or(Error::Shape { step: "orders", what: "no order list in the page" })?;
    let list: Value = serde_json::Deserializer::from_str(&data[at..])
        .into_iter::<Value>()
        .next()
        .and_then(std::result::Result::ok)
        .ok_or(Error::Shape { step: "orders", what: "order list is not JSON" })?;
    let orders = list["orders"].as_array().ok_or(Error::Shape { step: "orders", what: "orders is not a list" })?;
    let mut out = Vec::new();
    for o in orders {
        let ordered: String = s(&o["createDate"]).chars().take(10).collect();
        for li in o["orderLineItems"].as_array().into_iter().flatten() {
            let status = s(&li["orderLineItemStatus"]);
            let hold = s(&li["holdType"]);
            let stage = if (!hold.is_empty() && hold != "NONE") || !li["cancelType"].is_null() {
                Stage::Held
            } else if status == "COMPLETE" {
                Stage::Sent
            } else {
                Stage::Open
            };
            let mut shown = status.clone();
            if stage == Stage::Held {
                shown = format!("{status} (hold {hold}, cancel {})", li["cancelType"]);
            }
            out.push(Order {
                via: "parchment",
                id: s(&li["deliveryId"]),
                from: s(&li["student"]["sendingMemberName"]),
                to: s(&li["recipient"]["address"]["destinationName"]),
                status: shown,
                ordered: ordered.clone(),
                changed: None,
                stage,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "tests/unit.rs"]
mod tests;
