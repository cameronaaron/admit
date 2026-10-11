//! The applicant's profile as declared in `profile/*.toml`, independent of any
//! portal, plus the checks that hold for every portal.
//!
//! A profile is the source of truth. Portal adapters (`admit-liaison`, …)
//! diff it against what the portal holds and converge the portal to it;
//! nothing outside the profile is ever deleted by them.

pub mod finding;
pub mod journal;
pub mod orders;
pub mod spec;
pub mod validate;

pub use finding::Finding;
pub use spec::*;
