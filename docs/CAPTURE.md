# Capture contract (work sqlite)

Decision: **capture Schedule 13D / 13G accessions, not filing bodies.** Work sqlite is `{--db}` (prod `/var/lib/edgar-13d/edgar-13d.sqlite`). Logical name `edgar-13d`. There is no published `current/` copy. The collector watches work sqlite only. Warehouse views are not applied in this pass.

Canonical contract: [capturable-state design principles](https://github.com/alexwoolford/capturable-state/blob/main/docs/design-principles.md) §0 / §7 and [datetime.md](https://github.com/alexwoolford/capturable-state/blob/main/docs/datetime.md). Capture the trickle, not the hose.

Pin: `capturable-state` git tag `v0.1.1` (not a path dep; do not copy `src/*.rs`).

Spec: [mosaic docs/research/EDGAR_13D.md](https://github.com/alexwoolford/mosaic/blob/main/docs/research/EDGAR_13D.md).

## What is captured

| Table / stream | Capture? | Mode | Why |
| --- | --- | --- | --- |
| `filings` | **yes** | after | Product. Key `accession` |
| `ingest_runs` | **yes** | after | Did last night finish? |
| Filing `.txt` bodies | **no** | — | Hose-adjacent |
| Item 4 / Item 5 prose, exhibits, CUSIP, per-person voting power | **no** | — | Not the capture set |
| Ticker | **no** | — | Join-time on issuer CIK via `warehouse.issuer` |
| `_outbox` | platform | — | Generated |

Identity: one row per `accession`. An amendment is a **new** accession. Identical rerun must not emit a new outbox row for `filings` (`ON CONFLICT … WHERE` any column differs). Soft-delete unused in v1.

The index CIK is `filer_cik`. Issuer CIK is the single `SUBJECT-COMPANY` block. Zero subjects, or more than one, is `filings_failed` and no row. If `FILED-BY` disagrees with the index CIK, the index CIK stays.

`percent_of_class` and `aggregate_shares` are TEXT, and NULL unless every Schedule 13D/13G XML reporting person states the same pair. Differing persons are not summed. Form 3/4/5 `<ownershipDocument>` is ignored. Missing XML leaves both columns NULL.

`filings_upserted` counts accession rows written this run, not index lines. `filings_seen` counts index rows kept (`SC 13D`, `SC 13D/A`, `SC 13G`, `SC 13G/A` only).

These are **labels**, not leads. A 13G is not a quieter 13D.

Do not `collect --snapshot` this database.

A missing daily `master.YYYYMMDD.idx` is 404, or 403 on Sat/Sun. Both are an empty successful ingest. Weekday index 403 is an error and stops a range. Filing `.txt` 403/404 increment `filings_failed`. Do not guess an issuer.

## Clocks

| Layer | Columns | Type |
| --- | --- | --- |
| Facts | `filed_date` | TEXT `YYYY-MM-DD` |
| Facts | run `started_at` / `finished_at` | TEXT `YYYY-MM-DDTHH:MM:SSZ` |
| Envelope | `_outbox.ts`, `deleted_at` | INTEGER Unix seconds |

## Announce / nudge

`install()` on work sqlite. Announce stem is `edgar-13d`. Host `ReadWritePaths` are a later mosaic pass. This crate does not install a timer.
