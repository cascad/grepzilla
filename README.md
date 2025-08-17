# Grepzilla — Hyperscale-ready Full‑Text/Wildcard Engine (README)

> Стартуем с правильной архитектуры (сегменты, префильтр по n‑gram, verify на regex), но сразу имеем **рабочий кусок**, который можно потрогать: сборка сегмента из JSONL и поиск по шаблону `*wildcard*` в одном сегменте.

---

## TL;DR (как быстро проверить руками)

```bash
# Cборка
cargo build --release

# Папки и данные
# Файл: examples/data.jsonl
./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000001

# Поиск по сегменту
./target/release/gzctl search-seg --seg segments/000001 --q "*играет*"
./target/release/gzctl search-seg --seg segments/000001 --q "*мяч*" --field text.body
# Вывод: ext_id\tdoc_id\tpreview
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
   ├─ grepzilla_segment/       # библиотека: сегмент V1 (JSON), нормализация, n‑gram
   │  ├─ Cargo.toml
   │  └─ src/
   │     ├─ lib.rs
   │     ├─ segjson.rs        # SegmentWriter/Reader (V1: JSON)
   │     ├─ gram.rs           # 3‑граммы и обязательные граммы из wildcard
   │     └─ normalizer.rs
   └─ gzctl/                   # CLI: build-seg, search-seg
      ├─ Cargo.toml
      └─ src/
         └─ main.rs
```

---

## Архитектура (high‑level)

* **Сегменты (immutable)** — минимальная единица хранения/поиска. В V1 — JSON файлы; в V2 → mmap + FST + Roaring‑блоки.
* **Префильтр** — пересечение Roaring‑битмапов по **обязательным 3‑граммам** (из запроса), опционально с `field mask`.
* **Verify** — строгая проверка шаблона (wildcard → regex) уже по тексту документа в `DocStore`.
* **LSM ingest (позже)** — WAL → memtable → маленькие сегменты → компакция в большие.
* **Шардинг (позже)** — по `(tenant, labels, time)`; брокер парсит запрос, назначает бюджеты, стримит ответы.

Такой разрез даёт: предсказуемый префильтр (дёшево и быстро), строгую корректность через verify, независимую эволюцию хранения и планировщика.

---

## Формат Segment V1 (временный, но с правильными границами)

Папка сегмента содержит три файла:

* **Файл: segments/<ID>/meta.json** — `{ version: 1, doc_count, gram_count }`
* **Файл: segments/<ID>/grams.json** — `{ trigram: [doc_id, ...], ... }`
* **Файл: segments/<ID>/docs.jsonl** — строки формата `StoredDoc { doc_id, ext_id, fields }`, где `fields` — только строковые поля (уже нормализованные: lower+NFKC+без диакритик).

Интерфейсы уже стабильные:

* `trait SegmentWriter { fn write_segment(input_jsonl, out_dir) }`
* `trait SegmentReader { fn open_segment(path) -> Self; fn prefilter(op, grams) -> Bitmap; fn get_doc(doc_id) -> Option<&StoredDoc> }`

> V2 заменит `grams.json` на `(grams.idx + grams.dat)` с FST‑лексиконом и оффсетами под Roaring‑блоки → моментальное открытие и низкая память.

---

## Как работает поиск (мазками)

1. **Нормализация** (см. `crates/grepzilla_segment/src/normalizer.rs`): lower → NFKC → удаление диакритик.
2. **Wildcard → обязательные граммы** (см. `crates/grepzilla_segment/src/gram.rs`): берём длинные литеральные фрагменты (≥3), раскладываем в 3‑граммы.
3. **Prefilter** (см. `segjson.rs::prefilter`): Roaring‑пересечения/объединения/andnot по граммам (AND/OR/NOT).
4. **Verify** (см. `crates/gzctl/src/main.rs`): wildcard компилируется в `regex`, проверка по нужному полю (если задано) или по всем.
5. **Пагинация**: итерация `doc_id` по возрастанию с `--offset/--limit`.

Почему быстро: тяжёлый regex не бьёт по всему корпусу — сперва жёсткая усечка по n‑gram, потом «дорогая» проверка только на кандидатах.

---

## Примеры запуска

### Построить сегмент

```bash
# Файл: examples/data.jsonl
./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000001
```

### Поиск без указания поля

```bash
./target/release/gzctl search-seg --seg segments/000001 --q "*кот*"
# ext_id\tdoc_id\tpreview
```

### Поиск c ограничением поля

```bash
./target/release/gzctl search-seg --seg segments/000001 --q "*мяч*" --field text.body
```

---

## Дизайн, ориентированный на гиперскейл

* **n‑gram по умолчанию 3** (2 для CJK можно включить локально). Это обеспечивает поддержку `*подстрока*` и большинства «простых» regex без backtracking.
* **Roaring** для postings: быстрые пересечения, компактность, стабильная латентность.
* **Field mask** (V2): `field_id -> Bitmap(docIds)` для «узких» запросов по полю уже на префильтре.
* **Budgets & Cursors** (в брокере, V2): для «толстых» запросов выдаём стриминг‑результаты в SLA, не перегревая кластер.
* **Сегменты иммутабельны**: простые снапшоты/бэкапы, понятные компакции, разделение IO.

---

## План эволюции (минимум боли)

1. **V2 Segment (mmap/FST/blocks)**

   * `grams.idx`: FST‑лексикон → оффсеты;
   * `grams.dat`: блоки Roaring с zstd, bloom‑на‑блок;
   * `docs.dat`: колоночный DocStore; meta с min/max по времени и DF‑гистограммами.
2. **Field mask** в сегменте и пересечение на префильтре.
3. **Позиции** (опционально) и BM25F‑ранжирование.
4. **Broker**: парсер/планировщик/шард‑маршрутизация/курсор и бюджеты.
5. **Ingest LSM**: WAL → memtable → flush/compaction; tombstones.

Интерфейсы в этом README уже под это заточены — апгрейды из V1 → V2 прозрачны для CLI и приложений.

---

## Конфиги и соглашения

* Нормализация: lower + NFKC + strip‑accents (плюс флаг на транслит при необходимости).
* Условно индексируем **все строковые поля**. Для production — mapping (например, `text.*`).
* Минимальная сила паттерна: требуется литеральная цепочка ≥3 символов; иначе отказ (политика защиты от full‑scan).

---

## Наблюдаемость (V2 план)

* Метрики: p50/p95/p99 по этапам (prefilter/verify/total), размер кандидатов, cache hit‑rate.
* Логи: AST запроса, выбранные граммы, бюджеты, маршрутизация и прогресс по сегментам.

---

## Лицензия

Apache 2.0 — свободное использование с сохранением авторских прав.  
Подробнее: [LICENSE](LICENSE).

---

### Быстрые ссылки на исходники

* Нормализация: `crates/grepzilla_segment/src/normalizer.rs`
* 3‑граммы и разбор шаблонов: `crates/grepzilla_segment/src/gram.rs`
* Writer/Reader сегмента V1: `crates/grepzilla_segment/src/segjson.rs`
* CLI: `crates/gzctl/src/main.rs`




