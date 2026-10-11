//! LACCD student portal (PeopleSoft Campus Solutions behind ADFS): sign in and
//! download the district unofficial transcript, `LAC_SR_TSRPT.pdf`.
//!
//! Flow, as captured from the browser on 2026-10-06:
//!
//! 1. `GET  sso.laccd.edu/adfs/ls/idpinitiatedsignon.aspx?loginToRp=csprd.laccd.edu`
//!    — the ADFS sign-in form (`UserName`, `Password`, `AuthMethod`).
//! 2. `POST` it with `AuthMethod=FormsAuthentication`; 302 back to the same
//!    URL, which now answers with an auto-submitting form carrying
//!    `SAMLResponse` to `csprd.laccd.edu/saml2/sp/acs/post`. A wrong password
//!    re-renders the sign-in form with `#errorText`. No MFA step was observed.
//! 3. `POST` the SAML form; 302 to `/psoftsso/PrincipalRequest`, another
//!    auto-submitting form (`RemoteUser`, `session`, `sso_ticket`, …) to
//!    `/psp/csprd/?cmd=start`. Posting it sets the PeopleSoft cookies.
//! 4. `GET` the "View Unofficial Transcript" component; read `ICSID` and
//!    `ICStateNum` from it.
//! 5. `POST ICAction=DERIVED_SSTSRPT_REPORT_REQUEST_PB` (report type `UNOFF`
//!    is preselected); the answer contains `window.open('…/LAC_SR_TSRPT.pdf')`.
//! 6. `GET` that URL: the PDF.
//!
//! Credentials stay with the caller (the CLI keeps them in the Keychain).

pub mod html;

use reqwest::Url;

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("LACCD {step}: transport: {source}")]
    Transport { step: &'static str, source: reqwest::Error },
    #[error("LACCD {step}: HTTP {status}")]
    Http { step: &'static str, status: u16 },
    #[error("LACCD sign-in rejected: {0}")]
    BadLogin(String),
    #[error("LACCD {step}: {what} (the portal changed, or an extra sign-in step appeared)")]
    Shape { step: &'static str, what: &'static str },
    #[error("LACCD transcript download is not a PDF (content-type {0:?})")]
    NotPdf(String),
    #[error("LACCD {step}: bad URL {url:?}")]
    Url { step: &'static str, url: String },
}

pub type Result<T> = std::result::Result<T, FetchError>;

/// Hosts, overridable for tests.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub sso: String,
    pub campus: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints { sso: "https://sso.laccd.edu".into(), campus: "https://csprd.laccd.edu".into() }
    }
}

pub struct Credentials {
    /// As typed on the sign-in page (e.g. the student ID; the captured form
    /// sent it domain-qualified as `escsso\<id>`).
    pub user: String,
    pub password: String,
}

const SIGN_IN: &str = "/adfs/ls/idpinitiatedsignon.aspx?loginToRp=csprd.laccd.edu";
const TRANSCRIPT_COMPONENT: &str = "/psc/csprd_5/EMPLOYEE/SA/c/SSR_STUDENT_ACAD_REC_FL.SSR_VW_UNOFF_TS_FL.GBL";
const TRANSCRIPT_START: &str =
    "?Page=SSR_VW_UNOFF_TS_FL&pslnkid=CS_SSR_VW_UNOFF_TS_FL_LNK&ICAJAXTrf=true&ICAJAX=1&ICMDTarget=start";
/// A browser user agent: the F5 front end in front of both hosts sees every
/// request, and the captured flow was a browser's.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

pub struct Portal {
    http: reqwest::Client,
    ep: Endpoints,
}

struct Page {
    url: Url,
    body: String,
}

impl Portal {
    pub fn new(ep: Endpoints) -> Portal {
        Portal {
            http: reqwest::Client::builder()
                .cookie_store(true)
                .user_agent(USER_AGENT)
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            ep,
        }
    }

    async fn page(&self, step: &'static str, rb: reqwest::RequestBuilder) -> Result<Page> {
        let resp = rb.send().await.map_err(|source| FetchError::Transport { step, source })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(FetchError::Http { step, status: status.as_u16() });
        }
        let url = resp.url().clone();
        let body = resp.text().await.map_err(|source| FetchError::Transport { step, source })?;
        Ok(Page { url, body })
    }

    fn resolve(step: &'static str, base: &Url, action: &str) -> Result<Url> {
        base.join(action).map_err(|_| FetchError::Url { step, url: action.into() })
    }

    async fn submit(&self, step: &'static str, base: &Url, form: &html::Form) -> Result<Page> {
        let to = Self::resolve(step, base, &form.action)?;
        self.page(step, self.http.post(to).form(&form.fields)).await
    }

    /// Steps 1–3: an authenticated PeopleSoft session in the cookie store.
    pub async fn sign_in(&self, cred: &Credentials) -> Result<()> {
        let start = format!("{}{SIGN_IN}", self.ep.sso);
        let login = self.page("sign-in page", self.http.get(&start)).await?;
        let mut form = html::form_with(&login.body, "UserName")
            .ok_or(FetchError::Shape { step: "sign-in page", what: "no UserName form" })?;
        form.fields = vec![
            ("UserName".into(), cred.user.clone()),
            ("Password".into(), cred.password.clone()),
            ("AuthMethod".into(), "FormsAuthentication".into()),
        ];
        let after = self.submit("sign-in", &login.url, &form).await?;
        let saml = match html::form_with(&after.body, "SAMLResponse") {
            Some(f) => f,
            None => {
                return Err(match html::login_error(&after.body) {
                    Some(msg) => FetchError::BadLogin(msg),
                    None => FetchError::Shape { step: "sign-in", what: "no SAMLResponse form after signing in" },
                });
            }
        };
        let principal = self.submit("SAML relay", &after.url, &saml).await?;
        let start = html::form_with(&principal.body, "sso_ticket")
            .ok_or(FetchError::Shape { step: "SAML relay", what: "no PeopleSoft start form" })?;
        let home = self.submit("PeopleSoft start", &principal.url, &start).await?;
        if home.url.host_str() != Url::parse(&self.ep.campus).ok().as_ref().and_then(Url::host_str) {
            return Err(FetchError::Shape { step: "PeopleSoft start", what: "did not land on the student portal" });
        }
        Ok(())
    }

    /// Steps 4–6, on a signed-in session.
    pub async fn unofficial_transcript(&self) -> Result<Vec<u8>> {
        let component = format!("{}{TRANSCRIPT_COMPONENT}", self.ep.campus);
        let page = self.page("transcript page", self.http.get(format!("{component}{TRANSCRIPT_START}"))).await?;
        let st = html::ic_state(&page.body)
            .ok_or(FetchError::Shape { step: "transcript page", what: "no ICSID/ICStateNum (signed out?)" })?;
        let state_num = st.state_num.to_string();
        let fields = [
            ("ICAJAX", "1"),
            ("ICNAVTYPEDROPDOWN", "0"),
            ("ICType", "Panel"),
            ("ICElementNum", "5"),
            ("ICStateNum", state_num.as_str()),
            ("ICAction", "DERIVED_SSTSRPT_REPORT_REQUEST_PB"),
            ("ICModelCancel", "0"),
            ("ICXPos", "0"),
            ("ICYPos", "0"),
            ("ResponsetoDiffFrame", "-1"),
            ("TargetFrameName", "None"),
            ("FacetPath", "None"),
            ("ICFocus", ""),
            ("ICSaveWarningFilter", "0"),
            ("ICChanged", "0"),
            ("ICSkipPending", "0"),
            ("ICAutoSave", "0"),
            ("ICResubmit", "0"),
            ("ICSID", st.sid.as_str()),
            ("ICActionPrompt", "false"),
            ("DERIVED_SSTSRPT_TSCRPT_TYPE3", "UNOFF"),
        ];
        let resp = self.page("transcript request", self.http.post(&component).form(&fields)).await?;
        let url = html::report_url(&resp.body)
            .ok_or(FetchError::Shape { step: "transcript request", what: "no report link in the answer" })?;
        let url = Self::resolve("transcript download", &resp.url, &url)?;
        let pdf = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|source| FetchError::Transport { step: "transcript download", source })?;
        if !pdf.status().is_success() {
            return Err(FetchError::Http { step: "transcript download", status: pdf.status().as_u16() });
        }
        let ctype = pdf
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let bytes =
            pdf.bytes().await.map_err(|source| FetchError::Transport { step: "transcript download", source })?;
        if !bytes.starts_with(b"%PDF") {
            return Err(FetchError::NotPdf(ctype));
        }
        Ok(bytes.to_vec())
    }
}

/// Signs in and downloads the unofficial transcript.
pub async fn fetch_unofficial_transcript(ep: Endpoints, cred: &Credentials) -> Result<Vec<u8>> {
    let p = Portal::new(ep);
    p.sign_in(cred).await?;
    p.unofficial_transcript().await
}
