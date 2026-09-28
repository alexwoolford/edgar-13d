//! Nightly EDGAR Schedule 13D / 13G beneficial-ownership labels.
//! A label, not a lead: the crate does not emit a score and does not store a ticker.

pub mod db;
pub mod filing;
pub mod header;
pub mod http;
pub mod index;
pub mod ingest;
pub mod sec_ua;
pub mod time;
pub mod xml;

pub use db::{open, open_work, WorkDb, DB_NAME};
pub use ingest::{ingest_dates, ingest_day, IngestStats};
pub use sec_ua::{validate_user_agent, UserAgentError};
pub use time::{utc_iso, INSTANT_FMT};

pub(crate) fn pad_cik(cik: &str) -> String {
    let digits: String = cik.chars().filter(|c| c.is_ascii_digit()).collect();
    format!("{digits:0>10}")
}
