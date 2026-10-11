//! National Student Clearinghouse transcript orders
//! (`tsorder.studentclearinghouse.org`): read an order's status.
//!
//! There are no accounts. Tracking, as captured on 2026-10-10
//! (`captures/clearinghouse/track/`) and run live the same day:
//!
//! 1. `POST /api/tsorder/trackorder` with `{"email", "toOrderId"}` (the
//!    number as a string). The answer is always HTTP 200: `"status":
//!    "success"` with the order summary and an `Authorization-Token:
//!    Bearer …` header, or `"status": "fail"` with a `message` ("The entered
//!    information does not match", "… not found for a completed request").
//! 2. `GET /api/tsorder/trackorder/{id}/recipient` with that bearer: each
//!    recipient's current status and history.
//!
//! The summary also carries the student's full SSN and address; only the
//! school name and order date are read from it, and nothing is kept.
//!
//! Status codes seen on one order (Connecticut College to Azusa Pacific,
//! 2026-10-10): `P` Order Placed, `L` Paperless Consent Form Received, `V`
//! In Process at School, `ETS` Electronic Transcript Sent. Only `ETS` is
//! known to mean delivered; any other code is reported as still open, in
//! the Clearinghouse's own words. A recipient with a `multipleHoldInfo`
//! entry is held.

use admit_domain::orders::{Order, Stage};
use serde_json::Value;

pub const BASE: &str = "https://tsorder.studentclearinghouse.org";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Clearinghouse {step}: transport: {source}")]
    Transport { step: &'static str, source: reqwest::Error },
    #[error("Clearinghouse {step}: HTTP {status}")]
    Http { step: &'static str, status: u16 },
    #[error("Clearinghouse order {order}: {message}")]
    Refused { order: String, message: String },
    #[error("Clearinghouse {step}: {what} (the site changed?)")]
    Shape { step: &'static str, what: &'static str },
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Client {
    http: reqwest::Client,
    base: String,
}

impl Client {
    pub fn new(base: &str) -> Client {
        Client {
            // Checked live 2026-10-10: without a User-Agent the front end
            // answers 403, and the server reads only `Content-Type`
            // capitalised - hyper's lowercase `content-type` is 415.
            http: reqwest::Client::builder()
                .user_agent(concat!("admit/", env!("CARGO_PKG_VERSION")))
                .http1_title_case_headers()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            base: base.trim_end_matches('/').to_string(),
        }
    }

    async fn json(step: &'static str, rb: reqwest::RequestBuilder) -> Result<(reqwest::header::HeaderMap, Value)> {
        let resp = rb.send().await.map_err(|source| Error::Transport { step, source })?;
        if !resp.status().is_success() {
            return Err(Error::Http { step, status: resp.status().as_u16() });
        }
        let headers = resp.headers().clone();
        let body = resp.json().await.map_err(|source| Error::Transport { step, source })?;
        Ok((headers, body))
    }

    /// Every recipient of order `order`, placed with `email`.
    pub async fn track(&self, email: &str, order: &str) -> Result<Vec<Order>> {
        let api = format!("{}/api/tsorder", self.base);
        let (headers, summary) = Self::json(
            "track order",
            self.http
                .post(format!("{api}/trackorder"))
                .header("Origin", &self.base)
                .json(&serde_json::json!({ "email": email, "toOrderId": order })),
        )
        .await?;
        if summary["status"] != "success" {
            let message = summary["message"].as_str().unwrap_or("no message").to_string();
            return Err(Error::Refused { order: order.into(), message });
        }
        let token = headers
            .get("Authorization-Token")
            .and_then(|v| v.to_str().ok())
            .ok_or(Error::Shape { step: "track order", what: "no Authorization-Token header" })?;
        let (_, recipients) = Self::json(
            "order recipients",
            self.http.get(format!("{api}/trackorder/{order}/recipient")).header("Authorization", token),
        )
        .await?;
        orders(order, &summary, &recipients)
    }
}

/// `MM/DD/YYYY h:mm AM ET` → `YYYY-MM-DD`.
fn date(s: &str) -> Option<String> {
    let day = s.split_whitespace().next()?;
    let [m, d, y] = day.split('/').collect::<Vec<_>>()[..] else { return None };
    let ok = |s: &str, n| s.len() == n && s.bytes().all(|b| b.is_ascii_digit());
    (ok(m, 2) && ok(d, 2) && ok(y, 4)).then(|| format!("{y}-{m}-{d}"))
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().trim().to_string()
}

/// The two tracking answers as one order per recipient.
pub fn orders(order: &str, summary: &Value, recipients: &Value) -> Result<Vec<Order>> {
    let info = &summary["TrackOrderInfo"]["trackInfo"]["OrderSummary"];
    let from = text(&info["schoolInfo"]["schlName"]);
    let ordered = date(info["orderHeader"]["orderDate"].as_str().unwrap_or_default())
        .ok_or(Error::Shape { step: "track order", what: "no orderDate as MM/DD/YYYY" })?;
    let list = recipients["recipients"]
        .as_array()
        .ok_or(Error::Shape { step: "order recipients", what: "no recipients list" })?;
    Ok(list
        .iter()
        .map(|r| {
            let current = &r["statusInfo"]["current"];
            let held = current["multipleHoldInfo"].as_array().is_some_and(|h| !h.is_empty());
            let stage = match (held, current["status"].as_str()) {
                (true, _) => Stage::Held,
                (false, Some("ETS")) => Stage::Sent,
                _ => Stage::Open,
            };
            Order {
                via: "clearinghouse",
                id: order.to_string(),
                from: from.clone(),
                to: text(&r["organization"]),
                status: text(&current["statusTxt"]),
                ordered: ordered.clone(),
                changed: current["createdTimestamp"].as_str().and_then(date),
                stage,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_read_only_in_the_tracking_format() {
        assert_eq!(date("10/10/2026 4:50 PM ET").as_deref(), Some("2026-10-10"));
        for bad in ["", "2026-10-10", "10/10", "1/10/2026", "10/10/26", "10/10/2026/1", "aa/bb/cccc"] {
            assert_eq!(date(bad), None, "{bad:?}");
        }
    }
}
