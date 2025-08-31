# Grepzilla — Hyperscale-ready Full-Text/Wildcard Engine

> **Grepzilla** — движок полнотекстового поиска по `*подстроке*`/wildcard-шаблонам, с архитектурой под гиперскейл: сегменты, префильтр по n-gram, verify на regex/PCRE2, брокер с курсорами и дедупликацией.

---

## TL;DR (как быстро проверить руками)

```bash
# Сборка
cargo build --release

# Пример данных
# Файл: examples/data.jsonl
./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000001

# Поиск по сегменту
./target/release/gzctl search-seg --seg segments/000001 --q "*играет*"
./target/release/gzctl search-seg --seg segments/000001 --q "*мяч*" --field text.body
# Вывод: ext_id	doc_id	preview
```

---

## Workspace

```
grepzilla/
├─ Cargo.toml                  # workspace
├─ README.md                   # этот файл
├─ examples/
│  └─ data.jsonl
└─ crates/
   ├─ grepzilla_segment/       # библиотека: сегмент V1 (JSON), нормализация, n-gram
   │  └─ src/
   │     ├─ lib.rs
   │     ├─ segjson.rs        # SegmentWriter/Reader (V1: JSON)
   │     ├─ gram.rs           # 3-граммы и обязательные граммы из wildcard
   │     └─ normalizer.rs
   ├─ gzctl/                   # CLI: build-seg, search-seg
   │  └─ src/main.rs
   └─ broker/                  # HTTP API: /search, /manifest/:shard, /healthz
      └─ src/
         ├─ http_api.rs
         └─ search/{executor.rs,paginator.rs,mod.rs,types.rs}
```

---

## Архитектура (high-level)

* **Сегменты (immutable)** — минимальная единица хранения/поиска. В V1 — JSON; в V2 → mmap + FST + Roaring-блоки.
* **Prefilter** — пересечение Roaring-битмапов по обязательным 3-граммам.
* **Verify** — строгая проверка по regex/PCRE2.
* **Broker** — агрегация по сегментам, дедуп по `ext_id`, курсорная пагинация.
* **LSM ingest (roadmap)** — WAL → memtable → flush/compaction.
* **Шардинг (roadmap)** — `manifest.json` + `pin_gen` для стабильного повторного поиска.

---

## API

### POST /search

Запрос c указанием сегментов:

```json
{
  "wildcard": "*error*",
  "field": "text.body",
  "segments": ["segments/000001","segments/000002"],
  "page": { "size": 10, "cursor": null },
  "limits": { "parallelism": 2, "deadline_ms": 500, "max_candidates": 200000 }
}
```

Ответ:

```json
{
  "hits": [
    { "ext_id":"abc","doc_id":123,"matched_field":"text.body","preview":"...error..." }
  ],
  "cursor": {
    "per_seg": {
      "segments/000001": { "last_docid": 345 },
      "segments/000002": { "last_docid": 78 }
    },
    "pin_gen": { "0": 7 }
  },
  "metrics": {
    "candidates_total": 42,
    "time_to_first_hit_ms": 3,
    "deadline_hit": false,
    "saturated_sem": 0,
    "dedup_dropped": 1,
    "prefilter_ms": 13,
    "verify_ms": 8,
    "prefetch_ms": 2,
    "warmed_docs": 5
  }
}
```

#### По шардам (через манифест)

```json
{
  "wildcard": "*error*",
  "shards": [0,1],
  "page": { "size": 10, "cursor": null }
}
```

---

### GET /manifest/:shard

Возвращает список сегментов для шарда:

```json
{ "shard": 0, "gen": 7, "segments": ["segments/000001","segments/000002"] }
```

### GET /healthz

```json
{ "status": "ok" }
```

---

## Переменные окружения

### `GZ_MANIFEST`

Путь к `manifest.json`:

```json
{
  "shards": { "0": 7 },
  "segments": { "0:7": ["segments/000001","segments/000002"] }
}
```

### `GZ_VERIFY`

Выбор движка верификации:

* `regex` (по умолчанию)
* `pcre2`

Пример:

```bash
export GZ_VERIFY=pcre2
# Windows PowerShell:
$env:GZ_VERIFY = "pcre2"
```

---

## Примеры PowerShell (Windows)

### POST /search (сегменты)

```powershell
$body = @{
  wildcard = "*игра*"
  segments = @("segments/000001")
  page     = @{ size = 5; cursor = $null }
} | ConvertTo-Json -Depth 6

Invoke-RestMethod -Method POST `
  -Uri "http://localhost:8080/search" `
  -ContentType "application/json" `
  -Body $body | ConvertTo-Json -Depth 8
```

### POST /search (по шардам)

```powershell
$env:GZ_MANIFEST = "C:\work\grepzilla\manifest.json"

$body = @{
  wildcard = "*error*"
  shards   = @(0)
  page     = @{ size = 10; cursor = $null }
} | ConvertTo-Json -Depth 6

Invoke-RestMethod -Method POST `
  -Uri "http://localhost:8080/search" `
  -ContentType "application/json" `
  -Body $body | ConvertTo-Json -Depth 8
```

### Продолжение со второй страницы

```powershell
$cursor = ($resp | ConvertTo-Json -Depth 8 | ConvertFrom-Json).cursor

$body2 = @{
  wildcard = "*error*"
  segments = @("segments/000001")
  page     = @{ size = 10; cursor = $cursor }
} | ConvertTo-Json -Depth 10

Invoke-RestMethod -Method POST `
  -Uri "http://localhost:8080/search" `
  -ContentType "application/json" `
  -Body $body2 | ConvertTo-Json -Depth 10
```

---

## Быстрый старт брокера

```bash
# Linux/macOS
export GZ_MANIFEST=manifest.json
cargo run -p broker --release
```

```powershell
# Windows PowerShell
$env:GZ_MANIFEST = "manifest.json"
cargo run -p broker --release
```

Проверка liveness:

```bash
curl http://localhost:8080/healthz
```

---

## Лицензия

Apache 2.0 — свободное использование с сохранением авторских прав.  
Подробнее: [LICENSE](LICENSE).

---

## Быстрые ссылки на исходники

* Нормализация: `crates/grepzilla_segment/src/normalizer.rs`
* 3-граммы: `crates/grepzilla_segment/src/gram.rs`
* Segment V1: `crates/grepzilla_segment/src/segjson.rs`
* CLI: `crates/gzctl/src/main.rs`
* Broker API: `crates/broker/src/http_api.rs`
