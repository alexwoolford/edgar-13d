# edgar-13d

Nightly **Schedule 13D / 13G** beneficial-ownership labels from EDGAR. A **label**, not a leading observable. One row per accession. This crate does not store a ticker and does not parse Item 4 or Item 5 prose.

Capture contract: [docs/CAPTURE.md](docs/CAPTURE.md). Spec: [mosaic EDGAR_13D.md](https://github.com/alexwoolford/mosaic/blob/main/docs/research/EDGAR_13D.md).

```bash
export SEC_USER_AGENT='edgar-13d you@real-domain'
cargo run --release -- ingest --date 2026-09-11
cargo run --release -- status
cargo run --release -- lookup 0000320193
```

`cargo test` uses fixtures only. It does not need the network or a real User-Agent.

Ingest exits 0 only when `ingest_runs.status` is `ok`. `partial` and `error` exit 1.

Pin `capturable-state` git tag `v0.1.1`. Never `path = "../capturable-state"`.

Host: `cargo build --release` then `sudo ./deploy/install.sh`. Daily ops: [docs/DAILY_OPS.md](docs/DAILY_OPS.md). The timer is 09:00 UTC and is enabled without `--now`.
