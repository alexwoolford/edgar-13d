//! Schedule 13D/13G XML only. Not Form 3/4/5 `<ownershipDocument>`.
//!
//! A reporting person is an element with both `aggregateAmountOwned` and
//! `percentOfClass` as direct children (EDGAR Schedule 13D and 13G XML
//! Technical Specification). Store the pair when every such person states
//! the same values. Differing persons, or no XML, leave the columns NULL.
//! Do not sum.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipNumbers {
    Absent,
    Unanimous { shares: String, percent: String },
    Disagree,
}

pub fn ownership_numbers(body: &str) -> OwnershipNumbers {
    let mut pairs = Vec::new();
    for doc in xml_documents(body) {
        let Ok(tree) = roxmltree::Document::parse(doc) else {
            continue;
        };
        if tree.root_element().tag_name().name() == "ownershipDocument" {
            continue;
        }
        for node in tree.descendants().filter(|n| n.is_element()) {
            let Some(shares) = child_text(node, "aggregateAmountOwned") else {
                continue;
            };
            let Some(percent) = child_text(node, "percentOfClass") else {
                continue;
            };
            let shares = normalize_amount(&shares);
            let percent = normalize_amount(&percent);
            if shares.is_empty() || percent.is_empty() {
                continue;
            }
            pairs.push((shares, percent));
        }
    }
    match pairs.first() {
        None => OwnershipNumbers::Absent,
        Some(first) if pairs.iter().all(|pair| pair == first) => OwnershipNumbers::Unanimous {
            shares: first.0.clone(),
            percent: first.1.clone(),
        },
        Some(_) => OwnershipNumbers::Disagree,
    }
}

fn xml_documents(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("<XML>") {
        let after = &rest[start + "<XML>".len()..];
        if let Some(end) = after.find("</XML>") {
            out.push(after[..end].trim());
            rest = &after[end + "</XML>".len()..];
        } else {
            break;
        }
    }
    out
}

fn child_text(node: roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
        .and_then(|child| child.text())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn normalize_amount(raw: &str) -> String {
    raw.trim().replace([',', ' '], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unanimous_pair_is_stored_once() {
        match ownership_numbers(include_str!("../fixtures/sc13d-unanimous.txt")) {
            OwnershipNumbers::Unanimous { shares, percent } => {
                assert_eq!(shares, "1000000");
                assert_eq!(percent, "5.2");
            }
            other => panic!("expected unanimous, got {other:?}"),
        }
    }

    #[test]
    fn missing_xml_is_absent() {
        assert_eq!(
            ownership_numbers(include_str!("../fixtures/sc13g-no-xml.txt")),
            OwnershipNumbers::Absent
        );
    }

    #[test]
    fn differing_persons_are_not_summed() {
        assert_eq!(
            ownership_numbers(include_str!("../fixtures/sc13da-disagree.txt")),
            OwnershipNumbers::Disagree
        );
    }
}
