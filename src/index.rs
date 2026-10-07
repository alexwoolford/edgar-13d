//! EDGAR daily master index (`master.YYYYMMDD.idx`).
//! Schedule 13D / 13G and their amendments only.

use chrono::{Datelike, NaiveDate};

use crate::pad_cik;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRow {
    pub cik: String,
    pub company_name: String,
    pub form: String,
    pub filed_date: String,
    pub filename: String,
}

pub fn master_index_url(date: NaiveDate) -> String {
    let year = date.year();
    let qtr = (date.month() - 1) / 3 + 1;
    let ymd = date.format("%Y%m%d");
    format!("https://www.sec.gov/Archives/edgar/daily-index/{year}/QTR{qtr}/master.{ymd}.idx")
}

pub fn is_schedule_13dg(form: &str) -> bool {
    // The daily index prints both the short name and `SCHEDULE 13D` / `SCHEDULE 13G`.
    matches!(
        form.trim().to_ascii_uppercase().as_str(),
        "SC 13D"
            | "SC 13D/A"
            | "SC 13G"
            | "SC 13G/A"
            | "SCHEDULE 13D"
            | "SCHEDULE 13D/A"
            | "SCHEDULE 13G"
            | "SCHEDULE 13G/A"
    )
}

/// True when `body` is a daily master index, not an interstitial or empty 200.
pub fn has_cik_header(body: &str) -> bool {
    body.lines().any(|line| line.starts_with("CIK|"))
}

pub fn parse_master_index(body: &str) -> Vec<IndexRow> {
    let mut rows = Vec::new();
    let mut in_table = false;
    for line in body.lines() {
        if line.starts_with("CIK|") {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        if line.chars().all(|c| c == '-' || c.is_whitespace()) {
            continue;
        }
        let Some(row) = parse_index_line(line) else {
            continue;
        };
        if is_schedule_13dg(&row.form) {
            rows.push(row);
        }
    }
    rows
}

fn parse_index_line(line: &str) -> Option<IndexRow> {
    let mut parts = line.splitn(5, '|');
    let cik = pad_cik(parts.next()?.trim());
    let company_name = parts.next()?.trim().to_string();
    let form = parts.next()?.trim().to_string();
    let filed_raw = parts.next()?.trim();
    let filename = parts.next()?.trim().to_string();
    if cik.chars().all(|c| c == '0') || filename.is_empty() {
        return None;
    }
    let filed_date = normalize_date(filed_raw)?;
    Some(IndexRow {
        cik,
        company_name,
        form,
        filed_date,
        filename,
    })
}

pub fn normalize_date(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 8 {
        return Some(format!(
            "{}-{}-{}",
            &digits[0..4],
            &digits[4..6],
            &digits[6..8]
        ));
    }
    if raw.len() == 10 && raw.as_bytes().get(4) == Some(&b'-') {
        return Some(raw.to_string());
    }
    None
}

pub fn normalize_accession(s: &str) -> String {
    let d: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if d.len() == 18 {
        format!("{}-{}-{}", &d[0..10], &d[10..12], &d[12..18])
    } else {
        s.split_whitespace().collect()
    }
}

pub fn accession_from_filename(filename: &str) -> String {
    let base = filename.rsplit('/').next().unwrap_or(filename);
    let stem = base.strip_suffix(".txt").unwrap_or(base);
    normalize_accession(stem)
}

/// One group per accession, in first-seen order. The index lists the holder and the issuer.
pub fn group_by_accession(rows: Vec<IndexRow>) -> Vec<Vec<IndexRow>> {
    let mut grouped: Vec<(String, Vec<IndexRow>)> = Vec::new();
    for row in rows {
        let accession = accession_from_filename(&row.filename);
        if let Some((_, group)) = grouped.iter_mut().find(|(key, _)| key == &accession) {
            group.push(row);
        } else {
            grouped.push((accession, vec![row]));
        }
    }
    grouped.into_iter().map(|(_, group)| group).collect()
}

/// The duplicate index line is the issuer. The filer is the other line.
/// Several non-issuer lines resolve only when one matches `FILED-BY`.
pub fn choose_filer<'a>(
    group: &'a [IndexRow],
    issuer_cik: &str,
    filed_by_cik: Option<&str>,
) -> Option<&'a IndexRow> {
    let others: Vec<&IndexRow> = group.iter().filter(|row| row.cik != issuer_cik).collect();
    match others.len() {
        0 => group.first(),
        1 => Some(others[0]),
        _ => filed_by_cik.and_then(|cik| others.into_iter().find(|row| row.cik == cik)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture_and_keeps_only_13d_13g() {
        let body = include_str!("../fixtures/master.idx");
        assert!(has_cik_header(body));
        let rows = parse_master_index(body);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].form, "SC 13D");
        assert_eq!(rows[0].cik, "0000902664");
        assert_eq!(rows[0].filed_date, "2026-09-11");
        assert_eq!(rows[1].form, "SC 13G");
        assert_eq!(rows[2].form, "SC 13D/A");
        assert_eq!(rows[3].form, "SC 13D");
        assert_eq!(rows[4].form, "SCHEDULE 13G/A");
        assert!(rows.iter().all(|r| is_schedule_13dg(&r.form)));
    }

    #[test]
    fn drops_13f_and_8k() {
        assert!(!is_schedule_13dg("SC 13F"));
        assert!(!is_schedule_13dg("SCHEDULE 13F"));
        assert!(!is_schedule_13dg("8-K"));
        assert!(!is_schedule_13dg("4"));
        assert!(is_schedule_13dg("sc 13g/a"));
        assert!(is_schedule_13dg("schedule 13d"));
        assert!(is_schedule_13dg("SCHEDULE 13G/A"));
    }
}
