//! The ADFS / PeopleSoft specifics of the login relay and the transcript
//! request, on top of the generic readers in `admit-html` (re-exported).

pub use admit_html::{Form, attr, form_with, forms, tag_end, unescape};

/// The ADFS sign-in page's error line ("Incorrect user ID or password…").
pub fn login_error(html: &str) -> Option<String> {
    let i = html.find("id=\"errorText\"").or_else(|| html.find("id='errorText'"))?;
    let open = html[i..].find('>')? + i + 1;
    let close = html[open..].find('<')? + open;
    Some(unescape(html[open..close].trim())).filter(|t| !t.is_empty())
}

/// PeopleSoft component state: every postback must echo the latest
/// `ICStateNum` and the page's `ICSID`, or the server answers with a
/// "page is no longer available" error instead of acting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcState {
    pub sid: String,
    pub state_num: u32,
}

/// Reads the state from a full page (hidden inputs) or an AJAX response
/// (`ICStateNum.value=N;` script). The highest number wins.
pub fn ic_state(page: &str) -> Option<IcState> {
    let sid = forms(page)
        .into_iter()
        .flat_map(|f| f.fields)
        .find(|(n, _)| n == "ICSID")
        .map(|(_, v)| v)
        .or_else(|| input_by_id(page, "ICSID"))?;
    let mut nums: Vec<u32> = Vec::new();
    let mut at = 0;
    while let Some(i) = page[at..].find("ICStateNum.value=").map(|i| i + at) {
        at = i + "ICStateNum.value=".len();
        let digits: String = page[at..].chars().take_while(char::is_ascii_digit).collect();
        nums.extend(digits.parse::<u32>().ok());
    }
    nums.extend(input_by_id(page, "ICStateNum").and_then(|v| v.parse::<u32>().ok()));
    Some(IcState { sid, state_num: nums.into_iter().max()? })
}

fn input_by_id(page: &str, id: &str) -> Option<String> {
    let lower = page.to_ascii_lowercase();
    let mut at = 0;
    while let Some(s) = lower[at..].find("<input").map(|i| i + at) {
        let e = tag_end(page, s).unwrap_or(page.len());
        let tag = &page[s..e];
        if attr(tag, "id").as_deref() == Some(id) {
            return attr(tag, "value");
        }
        at = e;
    }
    None
}

/// The report URL the "Submit" action opens in a new window:
/// `window.open('https://…/view/{V2}…/LAC_SR_TSRPT.pdf','','')`.
pub fn report_url(page: &str) -> Option<String> {
    let mut at = 0;
    while let Some(i) = page[at..].find("window.open(").map(|i| i + at) {
        at = i + "window.open(".len();
        let q = page[at..].chars().next()?;
        if q != '\'' && q != '"' {
            continue;
        }
        let inner = &page[at + 1..];
        let url = &inner[..inner.find(q)?];
        if url.to_ascii_lowercase().contains(".pdf") {
            return Some(unescape(url));
        }
    }
    None
}

#[cfg(test)]
#[path = "tests/html_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/html_malformed.rs"]
mod malformed;
