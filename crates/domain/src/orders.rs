//! Official transcript orders, whichever service placed them, checked
//! against `transcript_orders.toml`: every declared send needs an order,
//! and an order that is held needs a person.

use crate::{Finding, TranscriptOrdersFile, TranscriptSend};

/// The services `via` may name.
pub const SERVICES: [&str; 2] = ["clearinghouse", "parchment"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Placed and not yet delivered.
    Open,
    /// The service reports the transcript sent (or the order complete).
    Sent,
    /// Stopped until someone acts: a hold, a cancellation, a failed delivery.
    Held,
}

/// One transcript to one recipient. A Clearinghouse order with several
/// recipients is several of these, one id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub via: &'static str,
    pub id: String,
    pub from: String,
    pub to: String,
    /// The service's own wording.
    pub status: String,
    /// `YYYY-MM-DD` the order was placed.
    pub ordered: String,
    /// `YYYY-MM-DD` of the current status, when the service dates it.
    pub changed: Option<String>,
    pub stage: Stage,
}

impl Order {
    fn when(&self) -> String {
        match &self.changed {
            Some(d) => format!("on {d}"),
            None => format!("(ordered {})", self.ordered),
        }
    }
}

/// Names compared as the services print them differ only in case and
/// punctuation ("AZUSA PACIFIC UNIVERSITY", "California State University -
/// Northridge"); anything else must be written the service's way.
pub fn same_school(a: &str, b: &str) -> bool {
    let norm =
        |s: &str| s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect::<String>();
    norm(a) == norm(b)
}

fn matches(send: &TranscriptSend, o: &Order) -> bool {
    o.via == send.via
        && same_school(&send.from, &o.from)
        && same_school(&send.to, &o.to)
        && send.order.as_ref().is_none_or(|id| id.trim() == o.id)
}

/// Findings for every declared send, then a note for each order no send
/// accounts for. `orders` is what the services returned; a service that
/// could not be read is the caller's finding.
pub fn check(file: &TranscriptOrdersFile, orders: &[Order]) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut used = vec![false; orders.len()];
    for send in &file.sends {
        let label = format!("transcript {} -> {}", send.from, send.to);
        if !SERVICES.contains(&send.via.as_str()) {
            out.push(Finding::Violation(format!("{label}: via {:?} is not one of {SERVICES:?}", send.via)));
            continue;
        }
        let mine: Vec<usize> = (0..orders.len()).filter(|&i| matches(send, &orders[i])).collect();
        for &i in &mine {
            used[i] = true;
        }
        // The best news wins: one delivered copy is enough.
        let pick = |stage| {
            mine.iter().map(|&i| &orders[i]).filter(|o| o.stage == stage).max_by(|a, b| a.ordered.cmp(&b.ordered))
        };
        let found = match (pick(Stage::Sent), pick(Stage::Open), pick(Stage::Held)) {
            (Some(o), ..) => Finding::Note(format!("{label}: {} {} ({} {})", o.status, o.when(), o.via, o.id)),
            (None, Some(o), _) => Finding::Pending(format!("{label}: {} {} ({} {})", o.status, o.when(), o.via, o.id)),
            (None, None, Some(o)) => Finding::Violation(format!(
                "{label}: {} {} ({} {}) - held until someone acts on the service's order page",
                o.status,
                o.when(),
                o.via,
                o.id
            )),
            (None, None, None) => match (&send.order, send.via.as_str()) {
                (Some(id), _) => Finding::Violation(format!(
                    "{label}: {} order {id} not found with that sender and recipient",
                    send.via
                )),
                (None, "clearinghouse") => Finding::Pending(format!(
                    "{label}: not ordered (or its Clearinghouse order number is not recorded: order = \"...\")"
                )),
                (None, _) => Finding::Pending(format!("{label}: not ordered on {}", send.via)),
            },
        };
        out.push(found);
    }
    for (o, used) in orders.iter().zip(used) {
        if !used {
            out.push(Finding::Note(format!(
                "{} order {}: {} -> {} ({} {}) is not in transcript_orders.toml",
                o.via,
                o.id,
                o.from,
                o.to,
                o.status,
                o.when()
            )));
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/orders_tests.rs"]
mod tests;
