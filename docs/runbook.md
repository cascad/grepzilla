# Grepzilla Runbook

*Last updated: 2025‑08‑25*

This runbook explains how to build the workspace, run tests, prepare demo segments, start the broker, and verify everything via HTTP. Commands are shown for **Linux/macOS (bash)** and **Windows PowerShell**.

---

## 1) Build

**Release build (entire workspace)**

```bash
# Linux/macOS
cargo build --release --workspace
```

```powershell
# Windows (PowerShell)
cargo build --release --workspace
```

---

## 2) Tests

### Run all workspace tests

```bash
cargo test --workspace
```

### Only broker tests

```bash
cargo test -p broker
```

### Specific integration tests

```bash
# HTTP /search (in-memory)
cargo test -p broker --test search_http -- --nocapture

# Parallel coordinator (if present)
cargo test -p broker --test search_parallel -- --nocapture

# Ingest: WAL → segment (library call)
cargo test -p broker --test ingest_wal_roundtrip -- --nocapture
```

### With logs

```bash
RUST_LOG=info cargo test -p broker -- --nocapture
```

```powershell
$env:RUST_LOG="info"; cargo test -p broker -- --nocapture
```

---

## 3) Prepare segments for manual checks

Input: `examples/data.jsonl`

```bash
# Linux/macOS
./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000001
./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000002
```

```powershell
# Windows
.\target\release\gzctl.exe build-seg --input examples\data.jsonl --out segments\000001
.\target\release\gzctl.exe build-seg --input examples\data.jsonl --out segments\000002
```

### Quick sanity check of a segment (without broker)

```bash
cargo run -p gzctl -- search-seg --seg segments/000001 --q "*игра*" --field text.body --debug-metrics
```

---

## 4) Start the broker

Entry point: `crates/broker/src/main.rs` (wired to `SearchCoordinator` and `axum`).

```bash
cargo run -p broker
```

Expected log:

```
broker listening address=0.0.0.0:8080
```

To enable logs explicitly:

```bash
RUST_LOG=info cargo run -p broker
```

```powershell
$env:RUST_LOG="info"; cargo run -p broker
```

---

## 5) HTTP checks (manual)

> Request schema matches `crates/broker/src/search/types.rs`.

### 5.1 Single-segment search

```bash
curl -s -X POST http://localhost:8080/search \
  -H 'Content-Type: application/json' \
  -d '{
    "wildcard": "*игра*",
    "field": "text.body",
    "segments": ["segments/000001"],
    "page": { "size": 2, "cursor": null },
    "limits": { "parallelism": 4, "deadline_ms": 800, "max_candidates": 200000 }
  }' | jq .
```

```powershell
$seg  = (Resolve-Path ".\segments\000001").Path.Replace('\','\\')
$json = '{"wildcard":"*игра*","field":"text.body","segments":["' + $seg + '"],"page":{"size":2,"cursor":null},"limits":{"parallelism":2,"deadline_ms":1000,"max_candidates":100000}}'

$utf8 = New-Object System.Text.UTF8Encoding($false)   # false => без BOM
[IO.File]::WriteAllBytes("req.json", $utf8.GetBytes($json))

& "$env:SystemRoot\System32\curl.exe" -s -X POST "http://127.0.0.1:8080/search" `
  -H "Content-Type: application/json; charset=utf-8" `
  --data-binary "@req.json"

# {"hits":[{"doc_id":0,"ext_id":"1","matched_field":"text.body"},{"doc_id":1,"ext_id":"2","matched_field":"text.body"}],"cursor":{"per_seg":{"D:\\rust_repo\\grepzilla\\segments\\000001":{"last_docid":1}},"pin_gen":null},"metrics":{"candidates_total":2,"time_to_first_hit_ms":0,"deadline_hit":false,"saturated_sem":0}}

# взятый курсор из предыдущего

$cur = '{"per_seg":{"D:\\\\rust_repo\\\\grepzilla\\\\segments\\\\000001":{"last_docid":1}},"pin_gen":null}'
$seg  = (Resolve-Path ".\segments\000001").Path.Replace('\','\\')
$json = '{"wildcard":"*игра*","field":"text.body","segments":["' + $seg + '"],"page":{"size":2,"cursor":' + $cur + '},"limits":{"parallelism":2,"deadline_ms":1000,"max_candidates":100000}}'
$utf8 = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllBytes("req.json", $utf8.GetBytes($json))
& "$env:SystemRoot\System32\curl.exe" -s -X POST "http://127.0.0.1:8080/search" -H "Content-Type: application/json; charset=utf-8" --data-binary "@req.json"

# {"hits":[{"doc_id":0,"ext_id":"1","matched_field":"text.body"},{"doc_id":1,"ext_id":"2","matched_field":"text.body"}],"cursor":{"per_seg":{"D:\\rust_repo\\grepzilla\\segments\\000001":{"last_docid":1}},"pin_gen":null},"metrics":{"candidates_total":2,"time_to_first_hit_ms":0,"deadline_hit":false,"saturated_sem":0}}
```

Expected: `hits` (0..N), `cursor.per_seg["segments/000001"].last_docid`, `metrics`.

### 5.2 Next page (use `cursor` from previous response)

```bash
CUR=$(curl -s -X POST http://localhost:8080/search \
  -H 'Content-Type: application/json' \
  -d '{"wildcard":"*игра*","field":"text.body","segments":["segments\\000001"],"page":{"size":2,"cursor":null}}' \
  | jq -c '.cursor')

curl -s -X POST http://localhost:8080/search \
  -H 'Content-Type: application/json' \
  -d "{\
    \"wildcard\":\"*игра*\",\
    \"field\":\"text.body\",\
    \"segments\":[\"segments/000001\"],\
    \"page\":{\"size\":2,\"cursor\":$CUR}\
  }" | jq .
```

### 5.3 Multi-segment search

```bash
curl -s -X POST http://localhost:8080/search \
  -H 'Content-Type: application/json' \
  -d '{
    "wildcard": "*игра*",
    "field": "text.body",
    "segments": ["segments/000001","segments/000002"],
    "page": { "size": 10, "cursor": null },
    "limits": { "parallelism": 4, "deadline_ms": 800, "max_candidates": 200000 }
  }' | jq '{hits_len: (.hits|length), cursor}
```


```powershell
# req.json
{
  "wildcard": "*игра*",
  "field": "text.body",
  "segments": [
    "D:\\\\rust_repo\\\\grepzilla\\\\segments\\\\000001",
    "D:\\\\rust_repo\\\\grepzilla\\\\segments\\\\000002"
  ],
  "page": { "size": 10, "cursor": null },
  "limits": { "parallelism": 4, "deadline_ms": 1000, "max_candidates": 200000 }
}


& "$env:SystemRoot\System32\curl.exe" -s -X POST "http://127.0.0.1:8080/search" -H "Content-Type: application/json; charset=utf-8" --data-binary "@req.json"

# {"hits":[{"doc_id":0,"ext_id":"1","matched_field":"text.body"},{"doc_id":1,"ext_id":"2","matched_field":"text.body"},{"doc_id":0,"ext_id":"1","matched_field":"text.body"},{"doc_id":1,"ext_id":"2","matched_field":"text.body"}],"cursor":{"per_seg":{"D:\\\\rust_repo\\\\grepzilla\\\\segments\\\\000001":{"last_docid":1},"D:\\\\rust_repo\\\\grepzilla\\\\segments\\\\000002":{"last_docid":1}},"pin_gen":null},"metrics":{"candidates_total":4,"time_to_first_hit_ms":8,"deadline_hit":false,"saturated_sem":0}}
```

---

## 6) (Optional) Ingest batches

There is a **library** API now:

* file: `crates/broker/src/ingest/mod.rs`
* function: `handle_batch_json(records, &cfg)`

If you also expose an HTTP route `/ingest/batch`, you can test it like:

```bash
curl -s -X POST http://localhost:8080/ingest/batch \
  -H 'Content-Type: application/json' \
  -d '{"records":[{"_id":"a1","text":{"body":"foo"}},{"_id":"a2","text":{"body":"игра"}}]}'
```

Expected: `{ "ok": true, "appended": 2, "wal": "...", "segment": "segments/<id>" }`. You can then include that segment in `/search`.

---

## 7) Troubleshooting (quick)

* **`cannot find crate broker` in tests** → ensure `crates/broker/Cargo.toml` has:

  ```toml
  [lib]
  name = "broker"
  path = "src/lib.rs"
  ```

  and `src/lib.rs` re-exports modules:

  ```rust
  pub mod search;
  pub mod storage_adapter;
  pub mod config;
  pub mod http;
  pub mod ingest;
  ```

* **`.next()` not found for `FuturesUnordered`** → add `use futures::StreamExt;` in `search/executor.rs`.

* **`open_segment` not found** → add `use grepzilla_segment::SegmentReader;` in the file where you call `JsonSegmentReader::open_segment(..)`.

* **Lifetime issues with `spawn_blocking`** → don’t move `&reader` into `spawn_blocking`. Run prefilter/verify in the current async task, or recreate data inside the closure.

* **Weak wildcard (e.g., only `*`/`?`)** → prefilter requires ≥3 consecutive literal chars; strengthen the pattern or search may return zero candidates.

---

## 8) Repo map (for navigation)

* **Server / Broker**

  * `crates/broker/src/main.rs` — entrypoint
  * `crates/broker/src/http.rs` — `axum` router (`/search`)
  * `crates/broker/src/search/{mod.rs, types.rs, executor.rs, paginator.rs}` — coordinator, concurrency, cursors
  * `crates/broker/src/storage_adapter.rs` — single-segment search via `grepzilla_segment`
  * `crates/broker/src/ingest/{mod.rs, wal.rs, compactor.rs}` — ingest API (library); HTTP route optional
  * `crates/broker/tests/*.rs` — integration tests

* **Segment engine**

  * `crates/grepzilla_segment/src/segjson.rs` — V1 JSON segment writer/reader
  * `crates/grepzilla_segment/src/gram.rs` — trigrams & required grams
  * `crates/grepzilla_segment/src/normalizer.rs` — normalization

* **CLI**

  * `crates/gzctl` — `build-seg`, `search-seg`

---

## 9) Pre-commit checklist

* [ ] `cargo build --release --workspace`
* [ ] `cargo test -p broker`
* [ ] segment `segments/000001` built
* [ ] `cargo run -p broker`
* [ ] `/search` returns `hits` and `cursor`
* [ ] next page with cursor works (no duplicates)
* [ ] multi-segment search returns results and `cursor.per_seg` for each segment
