//! One accession. No ticker column.

use anyhow::{bail, Result};

use crate::header::parse_header;
use crate::index::{accession_from_filename, IndexRow};
use crate::xml::{ownership_numbers, OwnershipNumbers};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filing {
    pub accession: String,
    pub form: String,
    pub is_amendment: i64,
    pub filed_date: String,
    pub issuer_cik: String,
    pub issuer_name: Option<String>,
    pub filer_cik: String,
    pub filer_name: String,
    pub percent_of_class: Option<String>,
    pub aggregate_shares: Option<String>,
    pub filename: String,
    pub source: String,
}

pub fn filing_from_submission(body: &str, row: &IndexRow) -> Result<Filing> {
    let header = parse_header(body);
    if header.subjects.len() != 1 {
        bail!(
            "issuer CIK: expected one SUBJECT-COMPANY, found {}",
            header.subjects.len()
        );
    }
    let subject = &header.subjects[0];
    if subject.cik.chars().all(|c| c == '0') {
        bail!("issuer CIK missing");
    }
    let (aggregate_shares, percent_of_class) = match ownership_numbers(body) {
        OwnershipNumbers::Unanimous { shares, percent } => (Some(shares), Some(percent)),
        OwnershipNumbers::Absent | OwnershipNumbers::Disagree => (None, None),
    };
    let form = row.form.trim().to_string();
    Ok(Filing {
        accession: accession_from_filename(&row.filename),
        form: form.clone(),
        is_amendment: i64::from(form.to_ascii_uppercase().ends_with("/A")),
        filed_date: row.filed_date.clone(),
        issuer_cik: subject.cik.clone(),
        issuer_name: subject.name.clone(),
        filer_cik: row.cik.clone(),
        filer_name: row.company_name.clone(),
        percent_of_class,
        aggregate_shares,
        filename: row.filename.clone(),
        source: "txt".into(),
    })
}
