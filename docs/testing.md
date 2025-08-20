# Grepzilla — Как тестить (Epics A–E)

**Файл:** `docs/testing.md`

Этот документ — пошаговые инструкции для проверки работоспособности ключевых эпиков и задач. Все команды и пути привязаны к текущей структуре репозитория.

---

## 0) Подготовка окружения

```bash
# Файл: (корень репозитория)
cargo build --release
```

Проверочные данные:

* **Файл:** `examples/data.jsonl`

---

## EPIC A — Segment V1 (JSON), префильтр, verify

### A.1 Smoke / e2e руками

```bash
# Файлы/папки вывода будут созданы автоматически
./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000001

./target/release/gzctl search-seg --seg segments/000001 --q "*играет*"
./target/release/gzctl search-seg --seg segments/000001 --q "*мяч*" --field text.body --debug-metrics
```

**Ожидание:**

* Вывод в формате: `ext_id<TAB>doc_id<TAB>preview`.
* С `--debug-metrics`: видны `candidates_total`, `verified_total`, `hits_total`.
* При `--field` значение `candidates_total` **меньше**, чем без `--field` для того же запроса.

### A.2 Unit-тесты

**Файл:** `crates/grepzilla_segment/src/normalizer.rs`

* Добавить `#[cfg(test)]` с кейсами: регистр, NFKC, диакритики.

**Файл:** `crates/grepzilla_segment/src/gram.rs`

* Добавить `#[cfg(test)]` на `trigrams` (короткие строки, Unicode).

**Файл:** `crates/grepzilla_segment/src/segjson.rs`

* Добавить `#[cfg(test)]` на `prefilter` (AND/OR/NOT) с синтетическими данными.

Запуск:

```bash
# Файл: (корень репозитория)
cargo test --all
```

### A.3 Golden-тесты (интеграционные)

**Файлы:**

* `tests/fixtures/data_small.jsonl` — фикстура.
* `tests/golden/query1.out` — эталон вывода.
* `scripts/build_and_golden.sh` — скрипт.

**Пример скрипта:**

```bash
#!/usr/bin/env bash
set -euo pipefail

# Файл: scripts/build_and_golden.sh
./target/release/gzctl build-seg --input tests/fixtures/data_small.jsonl --out segments/test
./target/release/gzctl search-seg --seg segments/test --q "*кот*" > out1.txt

diff -u tests/golden/query1.out out1.txt
```

Запуск:

```bash
bash scripts/build_and_golden.sh
```

### A.4 Perf sanity (локально)

**Файл:** `scripts/gen_random_jsonl.py` (сделать при необходимости)

Шаги:

1. Сгенерировать `N=100k` документов.
2. `build-seg` и измерить время.
3. `search-seg` с селективным запросом, измерить p95.

---

## EPIC A2 — Field mask (ускорение поиска по полю)

### A2.1 Smoke + файл масок

```bash
./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000002
ls -lah segments/000002
```

**Ожидание:** в папке сегмента появился **файл:** `segments/000002/field_masks.json`.

Открыть и проверить, что есть ключи `text.body`, `text.title` с массивами doc\_id.

### A2.2 Prefilter с маской (сравнение кандидатов)

```bash
./target/release/gzctl search-seg --seg segments/000002 --q "*игра*" --debug-metrics
./target/release/gzctl search-seg --seg segments/000002 --q "*игра*" --field text.body --debug-metrics
```

**Ожидание:** второе число `candidates_total` **меньше**.

---

## EPIC A3 — Preview + подсветка

```bash
./target/release/gzctl search-seg --seg segments/000001 --q "*играет*"
```

**Ожидание:** в третьей колонке сниппет вида `... [играет] ...`.

* Если `text.body` отсутствует — fallback к `text.title`.

---

## EPIC A4 — Мини-метрики

```bash
./target/release/gzctl search-seg --seg segments/000001 --q "*кот*" --debug-metrics
```

**Ожидание:**

* Печать полей `candidates_total`, `verified_total`, `hits_total`.
* Инвариант: `hits_total ≤ verified_total ≤ candidates_total`.

---

## EPIC B — Manifest, Cursor (pin gen), Demo-broker

### B1 — Типы манифеста/пойнтера (unit)

**Файл:** `crates/grepzilla_segment/src/manifest.rs`

* Тест round-trip: JSON из RFC → struct → JSON (поля совпадают).

Запуск:

```bash
cargo test -p grepzilla_segment manifest
```

### B2 — ManifestStore (mock)

**Файл:** `crates/grepzilla_segment/src/manifest_store.rs`

* Тесты: `get_ptr`/`cas_ptr` (успех/провал при неверном `expected_gen`).

Запуск:

```bash
cargo test -p grepzilla_segment manifest_store
```

### B3 — Cursor model

**Файл:** `crates/grepzilla_segment/src/cursor.rs`

* Тесты: сериализация/десериализация, стабильность `matcher_hash`.

Запуск:

```bash
cargo test -p grepzilla_segment cursor
```

### B4 — Demo-broker (HTTP)

**Файл:** `crates/broker/src/main.rs`

Поднять:

```bash
cargo run -p broker
```

Первый запрос:

```bash
curl -s -X POST http://localhost:8080/search \
  -H 'Content-Type: application/json' \
  -d '{
    "wildcard":"*игра*",
    "field": null,
    "page": {"size": 2, "cursor": null}
  }' | jq .
```

**Ожидание:** есть `hits[]` и объект `cursor` с `pin_gen`.

Следующая страница:

```bash
# Подставь cursor из предыдущего ответа
curl -s -X POST http://localhost:8080/search \
  -H 'Content-Type: application/json' \
  -d '{
    "wildcard":"*игра*",
    "field": null,
    "page": {"size": 2, "cursor": { ... }}
  }' | jq .
```

**Ожидание:** другие `hits`, `pin_gen` тот же.

Проверка стабильности при смене manifest:

* Заменить файл **`manifest.json`** на версию `gen+1`.
* Повторить второй запрос с тем же `cursor` — выдача идёт по старому `gen` (не ломается).

---

## EPIC C — Абстракции API для Segment V2

**Файл:** `crates/grepzilla_segment/src/api.rs`

* Убедиться, что `gzctl` использует trait’ы `SegmentReader/Writer`.

Проверка:

```bash
cargo build --release
./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000003
```

**Ожидание:** сборка и команды работают без изменений в `gzctl`.

---

## EPIC D — Verify Engine (замена движка)

**Файл:** `crates/grepzilla_segment/src/verify.rs`

* Тесты: `compile`/`is_match` (Unicode, `?`, `*`, `(?s)`), edge-cases.

Интеграция:

* В `crates/gzctl/src/main.rs` пробросить VerifyEngine через DI.

Проверка:

```bash
cargo test -p grepzilla_segment verify
./target/release/gzctl search-seg --seg segments/000001 --q "*играет*"
```

**Ожидание:** поведение поиска не изменилось (golden‑тесты зелёные).

---

## EPIC E — Ingest (WAL + memtable + flush)

**Файлы:** `crates/ingest/{wal.rs,memtable.rs,flusher.rs}` (при появлении)

### E.1 Видимость до flush (L0)

1. Записать документ через мок‑ингест (API или прямой вызов в тесте).
2. Поиск через demo‑broker должен видеть документ **до** публикации нового `gen`.

### E.2 Fault‑injection

1. Смоделировать падение до flush.
2. После рестарта memtable пуст, но после реплея WAL документ снова виден.

---

## Примечания

* Все пути в командах указаны явно; при изменении структуры репозитория синхронизируйте их в этом файле.
* Для больших данных используйте генератор `scripts/gen_random_jsonl.py` (добавить при необходимости).
