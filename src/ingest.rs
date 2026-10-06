//! One UTC calendar day of Schedule 13D / 13G accessions.

use anyhow::{Context, Result};
use chrono::{Datelike, Days, NaiveDate, Utc, Weekday};

use crate::db::{upsert_filing, upsert_run, WorkDb};
use crate::filing::{filing_from_submission, Filing};
use crate::http::{filing_url, Fetcher};
use crate::index::{has_cik_header, master_index_url, parse_master_index, IndexRow};

#[derive(Debug, Clone, Default)]
pub struct IngestStats {
    pub status: String,
    pub filings_seen: i64,
    pub filings_upserted: i64,
    pub filings_failed: i64,
    pub txt_ok: i64,
    pub index_url: String,
}

pub fn ingest_day(
    db: &mut WorkDb,
    date: NaiveDate,
    fetcher: &mut dyn Fetcher,
) -> Result<IngestStats> {
    let started = Utc::now();
    let as_of = date.format("%Y-%m-%d").to_string();
    let index_url = master_index_url(date);
    let mut stats = IngestStats {
        index_url: index_url.clone(),
        status: "ok".into(),
        ..IngestStats::default()
    };

    let idx_resp = match fetcher.get(&index_url) {
        Ok(resp) => resp,
        Err(err) => {
            stats.status = "error".into();
            if let Err(run_err) = finish(db, &as_of, started, &stats) {
                tracing::error!(
                    error = %run_err,
                    "failed to record ingest_runs after index error"
                );
            }
            return Err(err).context("GET master index");
        }
    };
    if index_absent_is_ok(date, idx_resp.status) {
        tracing::info!(
            status = idx_resp.status,
            date = %as_of,
            "index missing; closed market"
        );
        finish(db, &as_of, started, &stats)?;
        return Ok(stats);
    }
    if idx_resp.status != 200 {
        stats.status = "error".into();
        finish(db, &as_of, started, &stats)?;
        anyhow::bail!("master index HTTP {} for {index_url}", idx_resp.status);
    }
    if !has_cik_header(&idx_resp.body) {
        stats.status = "error".into();
        finish(db, &as_of, started, &stats)?;
        anyhow::bail!("master index body has no CIK header for {index_url}");
    }

    let rows = parse_master_index(&idx_resp.body);
    stats.filings_seen = rows.len() as i64;

    for row in &rows {
        match resolve_filing(fetcher, row, &mut stats) {
            Ok(filing) => {
                if let Err(err) = persist_filing(db, &filing, &mut stats) {
                    stats.status = "error".into();
                    if let Err(run_err) = finish(db, &as_of, started, &stats) {
                        tracing::error!(
                            error = %run_err,
                            "failed to record ingest_runs after persist error"
                        );
                    }
                    return Err(err).context("persist filing");
                }
            }
            Err(err) => {
                tracing::warn!(
                    cik = %row.cik,
                    filename = %row.filename,
                    error = %err,
                    "filing failed"
                );
                stats.filings_failed += 1;
            }
        }
    }
    if stats.filings_failed > 0 {
        stats.status = "partial".into();
    }
    finish(db, &as_of, started, &stats)?;
    Ok(stats)
}

fn persist_filing(db: &mut WorkDb, filing: &Filing, stats: &mut IngestStats) -> Result<()> {
    let tx = db.unchecked_transaction()?;
    if upsert_filing(&tx, filing)? {
        stats.filings_upserted += 1;
    }
    tx.commit()?;
    Ok(())
}

fn finish(
    db: &mut WorkDb,
    as_of: &str,
    started: chrono::DateTime<Utc>,
    stats: &IngestStats,
) -> Result<()> {
    let tx = db.unchecked_transaction()?;
    upsert_run(
        &tx,
        as_of,
        started,
        Utc::now(),
        &stats.status,
        &stats.index_url,
        stats.filings_seen,
        stats.filings_upserted,
        stats.filings_failed,
        stats.txt_ok,
    )?;
    tx.commit()?;
    db.nudge.send();
    Ok(())
}

fn resolve_filing(
    fetcher: &mut dyn Fetcher,
    row: &IndexRow,
    stats: &mut IngestStats,
) -> Result<Filing> {
    let url = filing_url(&row.filename);
    let txt = fetcher.get(&url)?;
    if txt.status != 200 {
        anyhow::bail!("filing HTTP {} for {url}", txt.status);
    }
    let filing = filing_from_submission(&txt.body, row)?;
    stats.txt_ok += 1;
    Ok(filing)
}

/// Missing daily index: 404 always, or 403 on Sat/Sun UTC (OCI often 403s
/// unpublished weekend paths instead of 404). Weekday 403 is still an error.
fn index_absent_is_ok(date: NaiveDate, status: u16) -> bool {
    match status {
        404 => true,
        403 => matches!(date.weekday(), Weekday::Sat | Weekday::Sun),
        _ => false,
    }
}

pub fn parse_as_of(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").with_context(|| format!("date {s}"))
}

pub fn default_as_of() -> NaiveDate {
    Utc::now()
        .date_naive()
        .checked_sub_days(Days::new(1))
        .expect("yesterday")
}

pub fn inclusive_days(from: NaiveDate, to: NaiveDate) -> Result<Vec<NaiveDate>> {
    if from > to {
        anyhow::bail!("--from {from} is after --to {to}");
    }
    let mut days = Vec::new();
    let mut d = from;
    loop {
        days.push(d);
        if d == to {
            return Ok(days);
        }
        d = d.succ_opt().context("date overflow")?;
    }
}

pub fn ingest_dates(
    date: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<NaiveDate>> {
    match (date, from, to) {
        (None, None, None) => Ok(vec![default_as_of()]),
        (Some(d), None, None) => Ok(vec![parse_as_of(d)?]),
        (None, Some(f), Some(t)) => inclusive_days(parse_as_of(f)?, parse_as_of(t)?),
        (Some(_), _, _) => anyhow::bail!("--date cannot be combined with --from/--to"),
        _ => anyhow::bail!("--from and --to must both be set"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{last_run, lookup_filings, open_work, outbox_count, DB_NAME};
    use crate::http::{HttpResponse, MapFetcher};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV: Mutex<()> = Mutex::new(());

    struct TestDb {
        _lock: std::sync::MutexGuard<'static, ()>,
        _dir: TempDir,
        db: WorkDb,
    }

    fn test_db() -> TestDb {
        let lock = ENV.lock().unwrap_or_else(|p| p.into_inner());
        let dir = TempDir::new().unwrap();
        let announce = dir.path().join("announce");
        std::fs::create_dir_all(&announce).unwrap();
        std::env::set_var("STATE_CAPTURE_ANNOUNCE_DIR", announce.to_str().unwrap());
        let db = open_work(&dir.path().join("edgar-13d.sqlite")).unwrap();
        TestDb {
            _lock: lock,
            _dir: dir,
            db,
        }
    }

    fn fixture_fetcher() -> MapFetcher {
        let date = NaiveDate::from_ymd_opt(2026, 9, 11).unwrap();
        let mut urls = HashMap::new();
        urls.insert(
            master_index_url(date),
            HttpResponse {
                status: 200,
                body: include_str!("../fixtures/master.idx").into(),
            },
        );
        urls.insert(
            filing_url("edgar/data/902664/0000902664-26-000100.txt"),
            HttpResponse {
                status: 200,
                body: include_str!("../fixtures/sc13d-unanimous.txt").into(),
            },
        );
        urls.insert(
            filing_url("edgar/data/104169/0000104169-26-000070.txt"),
            HttpResponse {
                status: 200,
                body: include_str!("../fixtures/sc13g-no-xml.txt").into(),
            },
        );
        urls.insert(
            filing_url("edgar/data/789019/0000789019-26-000080.txt"),
            HttpResponse {
                status: 200,
                body: include_str!("../fixtures/sc13da-disagree.txt").into(),
            },
        );
        urls.insert(
            filing_url("edgar/data/1652044/0001652044-26-000050.txt"),
            HttpResponse {
                status: 200,
                body: include_str!("../fixtures/sc13d-two-subjects.txt").into(),
            },
        );
        urls.insert(
            filing_url("edgar/data/1067983/0001067983-26-000010.txt"),
            HttpResponse {
                status: 200,
                body: include_str!("../fixtures/sc13g-no-xml.txt").into(),
            },
        );
        MapFetcher { urls }
    }

    #[test]
    fn weekend_404_is_ok_zero_filings() {
        let mut t = test_db();
        let date = NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let url = master_index_url(date);
        let mut fetcher = MapFetcher {
            urls: HashMap::from([(
                url,
                HttpResponse {
                    status: 404,
                    body: "not found".into(),
                },
            )]),
        };
        let stats = ingest_day(&mut t.db, date, &mut fetcher).unwrap();
        assert_eq!(stats.status, "ok");
        assert_eq!(stats.filings_seen, 0);
        assert_eq!(last_run(&t.db).unwrap().unwrap().as_of_date, "2026-09-12");
    }

    #[test]
    fn weekend_403_is_ok_zero_filings() {
        let mut t = test_db();
        let date = NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let url = master_index_url(date);
        let mut fetcher = MapFetcher {
            urls: HashMap::from([(
                url,
                HttpResponse {
                    status: 403,
                    body: "forbidden".into(),
                },
            )]),
        };
        let stats = ingest_day(&mut t.db, date, &mut fetcher).unwrap();
        assert_eq!(stats.status, "ok");
        assert_eq!(stats.filings_seen, 0);
    }

    #[test]
    fn index_transport_error_records_status_error() {
        let mut t = test_db();
        let date = NaiveDate::from_ymd_opt(2026, 9, 11).unwrap();
        let mut fetcher = MapFetcher {
            urls: HashMap::new(),
        };
        let err = ingest_day(&mut t.db, date, &mut fetcher).unwrap_err();
        assert!(err.to_string().contains("GET master index"));
        assert_eq!(last_run(&t.db).unwrap().unwrap().status, "error");
        assert_eq!(last_run(&t.db).unwrap().unwrap().filings_seen, 0);
    }

    #[test]
    fn index_200_without_cik_header_is_error() {
        let mut t = test_db();
        let date = NaiveDate::from_ymd_opt(2026, 9, 11).unwrap();
        let url = master_index_url(date);
        let mut fetcher = MapFetcher {
            urls: HashMap::from([(
                url,
                HttpResponse {
                    status: 200,
                    body: "<html>access denied</html>".into(),
                },
            )]),
        };
        let err = ingest_day(&mut t.db, date, &mut fetcher).unwrap_err();
        assert!(err.to_string().contains("no CIK header"));
        assert_eq!(last_run(&t.db).unwrap().unwrap().status, "error");
        assert!(lookup_filings(&t.db, "320193").unwrap().is_empty());
    }

    #[test]
    fn weekday_403_is_error() {
        let mut t = test_db();
        let date = NaiveDate::from_ymd_opt(2026, 9, 11).unwrap();
        let url = master_index_url(date);
        let mut fetcher = MapFetcher {
            urls: HashMap::from([(
                url,
                HttpResponse {
                    status: 403,
                    body: "forbidden".into(),
                },
            )]),
        };
        let err = ingest_day(&mut t.db, date, &mut fetcher).unwrap_err();
        assert!(err.to_string().contains("HTTP 403"));
        assert_eq!(last_run(&t.db).unwrap().unwrap().status, "error");
    }

    #[test]
    fn ingest_fixture_keeps_index_filer_and_skips_unchanged_rerun() {
        let mut t = test_db();
        let date = NaiveDate::from_ymd_opt(2026, 9, 11).unwrap();
        let mut fetcher = fixture_fetcher();
        let stats = ingest_day(&mut t.db, date, &mut fetcher).unwrap();
        assert_eq!(stats.filings_seen, 5);
        assert_eq!(stats.txt_ok, 4);
        assert_eq!(stats.filings_failed, 1);
        assert_eq!(stats.filings_upserted, 4);
        assert_eq!(stats.status, "partial");

        let apple = lookup_filings(&t.db, "320193").unwrap();
        assert_eq!(apple.len(), 1);
        assert_eq!(apple[0].filer_cik, "0000902664");
        assert_eq!(apple[0].issuer_cik, "0000320193");
        assert_eq!(apple[0].percent_of_class.as_deref(), Some("5.2"));
        assert_eq!(apple[0].aggregate_shares.as_deref(), Some("1000000"));
        assert_eq!(apple[0].source, "txt");
        assert!(lookup_filings(&t.db, "999999").unwrap().is_empty());

        let passive = lookup_filings(&t.db, "0000104169-26-000070").unwrap();
        assert_eq!(passive.len(), 1);
        assert_eq!(passive[0].form, "SC 13G");
        assert!(passive[0].percent_of_class.is_none());
        assert!(passive[0].aggregate_shares.is_none());

        let amendment = lookup_filings(&t.db, "0000789019-26-000080").unwrap();
        assert_eq!(amendment.len(), 1);
        assert_eq!(amendment[0].is_amendment, 1);
        assert!(amendment[0].percent_of_class.is_none());
        assert!(amendment[0].aggregate_shares.is_none());

        let long_name = lookup_filings(&t.db, "0001067983-26-000010").unwrap();
        assert_eq!(long_name.len(), 1);
        assert_eq!(long_name[0].form, "SCHEDULE 13G/A");
        assert!(long_name[0].percent_of_class.is_none());

        let n1 = outbox_count(&t.db).unwrap();
        let mut fetcher = fixture_fetcher();
        let again = ingest_day(&mut t.db, date, &mut fetcher).unwrap();
        assert_eq!(again.filings_upserted, 0);
        let n2 = outbox_count(&t.db).unwrap();
        assert_eq!(n2, n1 + 1, "unchanged filings must not emit extra outbox");
    }

    #[test]
    fn db_name_and_announce_stem_match_repo() {
        let t = test_db();
        assert_eq!(DB_NAME, "edgar-13d");
        let _ = t;
        let announce_dir = std::env::var("STATE_CAPTURE_ANNOUNCE_DIR").unwrap();
        let announce = std::path::Path::new(&announce_dir).join(format!("{DB_NAME}.json"));
        assert!(
            announce.exists(),
            "announce file missing: {}",
            announce.display()
        );
    }
}
