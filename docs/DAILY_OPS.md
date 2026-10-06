# Daily ops (Oracle)

Oneshot + timer. The OS is the scheduler. Do not add an in-process cron.

Operator logs: `tracing` on stderr → journald (`SyslogIdentifier=edgar-13d-ingest`). Default `RUST_LOG=info`.

`ingest_runs` is capturable domain telemetry. Query it in mosaic.

## Layout

| | |
| --- | --- |
| Prefix | `/opt/edgar-13d` |
| State | `/var/lib/edgar-13d/edgar-13d.sqlite` |
| User | `edgar13d` (do not reuse `edgar` or `form4`) |
| Env | `/opt/edgar-13d/etc/edgar-13d.env` (`chmod 600`) |
| Timer | `edgar-13d-ingest.timer` **09:00 UTC** + 15m jitter, `Persistent=true` |
| Oneshot timeout | `TimeoutStartSec=1h` |

No published `current/`. Do not copy a laptop sqlite onto the host. No ticker column. No filing-body archive.

## Install

```bash
cargo build --release
sudo ./deploy/install.sh
# set SEC_USER_AGENT in /opt/edgar-13d/etc/edgar-13d.env
```

`install.sh` enables the timer **without** `--now`. First run: `sudo systemctl start edgar-13d-ingest.service`.

## Timer failed

1. `systemctl list-failed --no-pager`
2. `journalctl -u edgar-13d-ingest.service -n 80 --no-pager`
3. `edgar-13d --db /var/lib/edgar-13d/edgar-13d.sqlite status`
4. Re-run: `sudo systemctl start edgar-13d-ingest.service`

Do not hand-edit sqlite.

Weekend master-index **404 is success** (`status=ok`, zero filings). From this OCI IP an unpublished weekend path is often **403** rather than 404 — Sat/Sun 403 is the same success. Weekday index 403 is `status=error` (UA/Akamai), **including weekday US holidays**. Index 5xx and transport errors retry (four attempts, exponential backoff) then `status=error` (unit failed). An HTTP 200 body with no `CIK|` header is the same error, not an empty day.

A filing with no `SUBJECT-COMPANY`, or more than one, increments `filings_failed` and stores no row (`partial`, **exit 1**). A NULL `percent_of_class` is not a failure: the XML was absent or the reporting persons disagreed. Do not sum those persons.

## Backfill (`--from` / `--to`)

Not the first night. Invoke `run-ingest.sh` with `EDGAR_INGEST_FROM` / `EDGAR_INGEST_TO` outside the timer. An index error (weekday 403, transport failure, or a 200 body with no `CIK|` header) stops the loop.

## SEC fair access

Official: [Accessing EDGAR Data](https://www.sec.gov/search-filings/edgar-search-assistance/accessing-edgar-data).

- `SEC_USER_AGENT` sample shape: `edgar-13d you@real-domain`
- Refuse `example.com` and github-paren UAs (Akamai 403 undeclared bot)
- Default sleep 0.5s (~2 req/s). Ceiling 10 req/s. Do not rotate User-Agents (cap is per IP)
- This timer is 09:00 UTC so it does not share the 07:30 8-K burst or the 08:00 Form 4 burst on the same IP

Do not send `FAA_USER_AGENT` to SEC. Do not download `Feed/*.nc.tar.gz`.
