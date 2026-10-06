//! SGML submission header only. Issuer CIK is the subject company, not the index CIK.
//! Live headers print `SUBJECT COMPANY:` / `CENTRAL INDEX KEY:`. The tag form
//! `<SUBJECT-COMPANY>` still counts.

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
    let mut subjects: Vec<Subject> = blocks(header, "SUBJECT-COMPANY")
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
    let mut filed_by_cik = blocks(header, "FILED-BY")
        .into_iter()
        .find_map(|block| tag_value(block, "CIK"))
        .map(|cik| pad_cik(&cik));
    if subjects.is_empty() {
        let (colon_subjects, colon_filed_by) = colon_parties(header);
        subjects = colon_subjects;
        if filed_by_cik.is_none() {
            filed_by_cik = colon_filed_by;
        }
    }
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

#[derive(Clone, Copy)]
enum PartyKind {
    Subject,
    Filer,
}

struct OpenParty {
    kind: PartyKind,
    cik: Option<String>,
    name: Option<String>,
}

/// Colon labels from a complete-submission header. A later `SUBJECT COMPANY:`
/// or `FILED BY:` closes the block that is open.
fn colon_parties(header: &str) -> (Vec<Subject>, Option<String>) {
    let mut subjects = Vec::new();
    let mut filed_by_cik = None;
    let mut open: Option<OpenParty> = None;
    for line in header.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let upper = trimmed.to_ascii_uppercase();
        if upper == "SUBJECT COMPANY:" || upper == "FILED BY:" {
            close_party(&mut open, &mut subjects, &mut filed_by_cik);
            open = Some(OpenParty {
                kind: if upper == "SUBJECT COMPANY:" {
                    PartyKind::Subject
                } else {
                    PartyKind::Filer
                },
                cik: None,
                name: None,
            });
            continue;
        }
        let Some(party) = open.as_mut() else {
            continue;
        };
        if let Some(value) = label_value(trimmed, "CENTRAL INDEX KEY:") {
            party.cik = Some(pad_cik(value));
        } else if let Some(value) = label_value(trimmed, "COMPANY CONFORMED NAME:") {
            party.name = Some(value.to_string());
        }
    }
    close_party(&mut open, &mut subjects, &mut filed_by_cik);
    (subjects, filed_by_cik)
}

fn close_party(
    open: &mut Option<OpenParty>,
    subjects: &mut Vec<Subject>,
    filed_by_cik: &mut Option<String>,
) {
    let Some(party) = open.take() else {
        return;
    };
    match party.kind {
        PartyKind::Subject => {
            if let Some(cik) = party.cik {
                subjects.push(Subject {
                    cik,
                    name: party.name,
                });
            }
        }
        PartyKind::Filer => {
            if filed_by_cik.is_none() {
                *filed_by_cik = party.cik;
            }
        }
    }
}

fn label_value<'a>(trimmed: &'a str, label: &str) -> Option<&'a str> {
    let upper = trimmed.to_ascii_uppercase();
    let rest = upper.strip_prefix(label)?;
    let value = trimmed[trimmed.len() - rest.len()..].trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
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

    #[test]
    fn reads_colon_subject_company() {
        let header = parse_header(
            "<SEC-HEADER>
SUBJECT COMPANY:
COMPANY CONFORMED NAME: SOUTHWEST AIRLINES CO
CENTRAL INDEX KEY: 0000092380
FILED BY:
COMPANY CONFORMED NAME: PRIMECAP MANAGEMENT CO/CA/
CENTRAL INDEX KEY: 0000763212
</SEC-HEADER>
",
        );
        assert_eq!(header.subjects.len(), 1);
        assert_eq!(header.subjects[0].cik, "0000092380");
        assert_eq!(
            header.subjects[0].name.as_deref(),
            Some("SOUTHWEST AIRLINES CO")
        );
        assert_eq!(header.filed_by_cik.as_deref(), Some("0000763212"));
    }
}
