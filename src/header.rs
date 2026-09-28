//! SGML submission header only. Issuer CIK is `SUBJECT-COMPANY`, not the index CIK.

use crate::pad_cik;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub cik: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub subjects: Vec<Subject>,
    pub filed_by_cik: Option<String>,
}

pub fn parse_header(body: &str) -> Header {
    let header = header_slice(body);
    let subjects = blocks(header, "SUBJECT-COMPANY")
        .into_iter()
        .filter_map(|block| {
            let cik = tag_value(block, "CIK")?;
            let name = tag_value(block, "CONFORMED-NAME");
            Some(Subject {
                cik: pad_cik(&cik),
                name,
            })
        })
        .collect();
    let filed_by_cik = blocks(header, "FILED-BY")
        .into_iter()
        .find_map(|block| tag_value(block, "CIK"))
        .map(|cik| pad_cik(&cik));
    Header {
        subjects,
        filed_by_cik,
    }
}

fn header_slice(body: &str) -> &str {
    if let Some(i) = body.find("</SEC-HEADER>") {
        &body[..i]
    } else if let Some(i) = body.find("<DOCUMENT>") {
        &body[..i]
    } else {
        body
    }
}

fn blocks<'a>(header: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = header;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        if let Some(end) = after.find(&close) {
            out.push(&after[..end]);
            rest = &after[end + close.len()..];
        } else {
            break;
        }
    }
    out
}

fn tag_value(block: &str, tag: &str) -> Option<String> {
    let needle = format!("<{tag}>");
    let idx = block.find(&needle)?;
    let rest = &block[idx + needle.len()..];
    let end = rest.find(['<', '\n', '\r']).unwrap_or(rest.len());
    let value = rest[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_one_subject_and_filed_by() {
        let header = parse_header(include_str!("../fixtures/sc13d-unanimous.txt"));
        assert_eq!(header.subjects.len(), 1);
        assert_eq!(header.subjects[0].cik, "0000320193");
        assert_eq!(header.subjects[0].name.as_deref(), Some("APPLE INC"));
        assert_eq!(header.filed_by_cik.as_deref(), Some("0000999999"));
    }

    #[test]
    fn two_subjects_stay_two() {
        let header = parse_header(include_str!("../fixtures/sc13d-two-subjects.txt"));
        assert_eq!(header.subjects.len(), 2);
    }
}
