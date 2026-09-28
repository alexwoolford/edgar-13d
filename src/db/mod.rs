use std::ops::{Deref, DerefMut};
use std::path::Path;

use anyhow::{Context, Result};
use capturable_state::{
    apply_runtime_pragmas, install, CaptureConfig, CaptureMode, Nudge, TableSpec,
};
use rusqlite::{params, Connection, OptionalExtension};

use crate::filing::Filing;
use crate::time::utc_iso;

pub const DB_NAME: &str = "edgar-13d";

pub struct WorkDb {
    conn: Connection,
    pub nudge: Nudge,
}

impl Deref for WorkDb {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.conn
    }
}

impl DerefMut for WorkDb {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

fn open_conn(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
    }
    let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
    apply_runtime_pragmas(&conn)?;
    apply_schema(&conn)?;
    Ok(conn)
}

pub fn open(path: &Path) -> Result<Connection> {
    open_conn(path)
}

pub fn open_work(path: &Path) -> Result<WorkDb> {
    let conn = open_conn(path)?;
    let nudge = install_capture(&conn, path)?;
    Ok(WorkDb { conn, nudge })
}

fn apply_schema(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.execute_batch(include_str!("schema.sql"))
        .context("apply schema")?;
    Ok(())
}

fn install_capture(conn: &Connection, path: &Path) -> Result<Nudge> {
    let tables = [
        TableSpec::new("filings", CaptureMode::After),
        TableSpec::new("ingest_runs", CaptureMode::After),
    ];
    install(conn, &CaptureConfig::new(DB_NAME, path, &tables))
}

/// Change-aware upsert. Identical reruns emit no extra `_outbox` row.
pub fn upsert_filing(conn: &Connection, f: &Filing) -> Result<bool> {
    let n = conn.execute(
        "INSERT INTO filings (
            accession, form, is_amendment, filed_date, issuer_cik, issuer_name,
            filer_cik, filer_name, percent_of_class, aggregate_shares,
            filename, source, deleted_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL
         )
         ON CONFLICT(accession) DO UPDATE SET
            form = excluded.form,
            is_amendment = excluded.is_amendment,
            filed_date = excluded.filed_date,
            issuer_cik = excluded.issuer_cik,
            issuer_name = excluded.issuer_name,
            filer_cik = excluded.filer_cik,
            filer_name = excluded.filer_name,
            percent_of_class = excluded.percent_of_class,
            aggregate_shares = excluded.aggregate_shares,
            filename = excluded.filename,
            source = excluded.source
         WHERE filings.form IS NOT excluded.form
            OR filings.is_amendment IS NOT excluded.is_amendment
            OR filings.filed_date IS NOT excluded.filed_date
            OR filings.issuer_cik IS NOT excluded.issuer_cik
            OR filings.issuer_name IS NOT excluded.issuer_name
            OR filings.filer_cik IS NOT excluded.filer_cik
            OR filings.filer_name IS NOT excluded.filer_name
            OR filings.percent_of_class IS NOT excluded.percent_of_class
            OR filings.aggregate_shares IS NOT excluded.aggregate_shares
            OR filings.filename IS NOT excluded.filename
            OR filings.source IS NOT excluded.source",
        params![
            f.accession,
            f.form,
            f.is_amendment,
            f.filed_date,
            f.issuer_cik,
            f.issuer_name.as_deref(),
            f.filer_cik,
            f.filer_name,
            f.percent_of_class.as_deref(),
            f.aggregate_shares.as_deref(),
            f.filename,
            f.source,
        ],
    )?;
    Ok(n > 0)
}

#[allow(clippy::too_many_arguments)]
pub fn upsert_run(
    conn: &Connection,
    as_of_date: &str,
    started_at: chrono::DateTime<chrono::Utc>,
    finished_at: chrono::DateTime<chrono::Utc>,
    status: &str,
    index_url: &str,
    seen: i64,
    upserted: i64,
    failed: i64,
    txt_ok: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO ingest_runs (
            as_of_date, started_at, finished_at, status, index_url,
            filings_seen, filings_upserted, filings_failed, txt_ok
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(as_of_date) DO UPDATE SET
            started_at = excluded.started_at,
            finished_at = excluded.finished_at,
            status = excluded.status,
            index_url = excluded.index_url,
            filings_seen = excluded.filings_seen,
            filings_upserted = excluded.filings_upserted,
            filings_failed = excluded.filings_failed,
            txt_ok = excluded.txt_ok",
        params![
            as_of_date,
            utc_iso(started_at),
            utc_iso(finished_at),
            status,
            index_url,
            seen,
            upserted,
            failed,
            txt_ok,
        ],
    )?;
    Ok(())
}

#[derive(Debug)]
pub struct StatusRow {
    pub as_of_date: String,
    pub started_at: String,
    pub finished_at: String,
    pub status: String,
    pub filings_seen: i64,
    pub filings_upserted: i64,
    pub filings_failed: i64,
}

pub fn last_run(conn: &Connection) -> Result<Option<StatusRow>> {
    conn.query_row(
        "SELECT as_of_date, started_at, finished_at, status,
                filings_seen, filings_upserted, filings_failed
         FROM ingest_runs
         ORDER BY as_of_date DESC
         LIMIT 1",
        [],
        |r| {
            Ok(StatusRow {
                as_of_date: r.get(0)?,
                started_at: r.get(1)?,
                finished_at: r.get(2)?,
                status: r.get(3)?,
                filings_seen: r.get(4)?,
                filings_upserted: r.get(5)?,
                filings_failed: r.get(6)?,
            })
        },
    )
    .optional()
    .context("last ingest_runs")
}

pub fn lookup_filings(conn: &Connection, q: &str) -> Result<Vec<Filing>> {
    let q = q.trim();
    let cik = crate::pad_cik(q);
    let acc = crate::index::normalize_accession(q);
    let mut stmt = conn.prepare(
        "SELECT accession, form, is_amendment, filed_date, issuer_cik, issuer_name,
                filer_cik, filer_name, percent_of_class, aggregate_shares,
                filename, source
         FROM filings
         WHERE accession = ?1 OR issuer_cik = ?2 OR filer_cik = ?2
         ORDER BY filed_date DESC, accession",
    )?;
    let rows = stmt.query_map(params![acc, cik], |r| {
        Ok(Filing {
            accession: r.get(0)?,
            form: r.get(1)?,
            is_amendment: r.get(2)?,
            filed_date: r.get(3)?,
            issuer_cik: r.get(4)?,
            issuer_name: r.get(5)?,
            filer_cik: r.get(6)?,
            filer_name: r.get(7)?,
            percent_of_class: r.get(8)?,
            aggregate_shares: r.get(9)?,
            filename: r.get(10)?,
            source: r.get(11)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn outbox_count(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM _outbox", [], |r| r.get(0))
        .context("outbox count")
}
