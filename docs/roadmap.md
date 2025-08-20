# Grepzilla Roadmap — Phase 1 → Phase 2

> Этот файл описывает эпики и задачи, критерии приёмки и что править в коде. Использует RFC‑0001 как нормативный контекст.

## Шаблон карточки задачи

```
# [EPIC-ID]/[TASK-ID] — <краткий тайтл>

## Контекст
См. RFC-0001 (вставь сюда текст/ссылку). Вкратце: <1–3 предложения зачем эта задача>.

## Цель
<одно предложение, измеримый результат>

## Изменения в коде (пути файлов обязательно!)
- <путь/файл>: что добавить/изменить (пункты)
- <путь/файл>: ...

## API / CLI (если есть)
- <эндпоинт/флаг/формат>

## Acceptance Criteria
- [ ] пункт 1 (проверяемый)
- [ ] пункт 2
- [ ] пункт 3

## Тест-план
- [ ] unit: ...
- [ ] e2e/ручной: командой ... получаем ...

## Не входит в объём
- <вещи, которые явно не делаем сейчас>
```

---

## EPIC A: Segment V1 (JSON) — довести MVP до удобства

### A1 — Workspace и snake\_case имена

**Цель:** проект собирается, зависимости корректны.

**Изменения в коде:**

* `Cargo.toml` (workspace): `members = ["crates/grepzilla_segment","crates/gzctl"]`.
* `crates/gzctl/Cargo.toml`: заменить зависимость на `grepzilla_segment = { path = "../grepzilla_segment" }`.
* В исходниках: `use grepzilla_segment::...`.

**Acceptance:**

* [ ] `cargo build --release` проходит.
* [ ] `./target/release/gzctl build-seg ...` и `search-seg ...` работают.

---

### A2 — Field mask на префильтре

**Цель:** при `--field` пересекать bitmap с маской поля до verify.

**Изменения в коде:**

* `crates/grepzilla_segment/src/segjson.rs`:

  * Writer: собрать `field_masks.json` → `{ field: [doc_id...] }`.
  * Reader: грузить в `HashMap<String, Bitmap>`.
  * `fn prefilter(...)`: если указан `field`, `acc.and_inplace(field_mask)`.
* `crates/gzctl/src/main.rs`: проброс `--field` (если нужно в prefilter).

**Acceptance:**

* [ ] С `--field` кандидатов меньше (по отладочным метрикам).
* [ ] Итоговые хиты идентичны прежней версии.

**Тест-план:**

* [ ] unit на пересечение `field_mask`.
* [ ] ручной: на `examples/data.jsonl` запрос с `--field` быстрее по кандидатам.

---

### A3 — Улучшенный preview + подсветка

**Цель:** показывать сниппет из `text.body`/`text.title` с подсветкой совпадения.

**Изменения в коде:**

* `crates/gzctl/src/main.rs`: после `rx.is_match`, извлекать совпадения и собирать короткий сниппет `... [match] ...`.

**Acceptance:**

* [ ] В выводе присутствуют подсвеченные совпадения.

---

### A4 — Мини‑метрики поиска

**Цель:** видеть эффект префильтра/верификации.

**Изменения в коде:**

* `crates/gzctl/src/main.rs`: флаг `--debug-metrics`; печатать `candidates`, `verified`, `hits`.

**Acceptance:**

* [ ] При `--debug-metrics` числа выводятся и корректно меняются от запроса.

---

## EPIC B: Manifest (etcd) и курсоры (pin gen)

### B1 — Типы манифеста и указателя

**Цель:** сериализация/десериализация структур из RFC.

**Изменения в коде:**

* `crates/grepzilla_segment/src/manifest.rs` (новый): `ManifestPtr`, `ManifestV1`, `SegmentMeta`, `TombMeta`.
* `crates/grepzilla_segment/src/lib.rs`: `pub mod manifest;`.

**Acceptance:**

* [ ] JSON из RFC парсится и обратно сериализуется без потерь (unit).

---

### B2 — Хранилище указателя (mock)

**Цель:** интерфейс и in‑memory реализация для dev.

**Изменения в коде:**

* `crates/grepzilla_segment/src/manifest_store.rs` (новый):

  * `trait ManifestStore { fn get_ptr(shard: u64) -> Result<ManifestPtr>; fn cas_ptr(shard: u64, expected_gen: u64, new_ptr: &ManifestPtr) -> Result<()> }`.
  * `struct InMemoryManifestStore { ... }`.

**Acceptance:**

* [ ] unit: `get_ptr`/`cas_ptr` работают, CAS проверяет `expected_gen`.

---

### B3 — Модель курсора

**Цель:** структура курсора как в RFC.

**Изменения в коде:**

* `crates/grepzilla_segment/src/cursor.rs` (новый): `SearchCursor` + `ShardPos` + `Budgets`.

**Acceptance:**

* [ ] сериализация/десериализация; `matcher_hash` считается из запроса.

---

### B4 — Demo‑broker (HTTP) с курсорами и pin gen

**Цель:** минимальный брокер, который выполняет поиск через сегменты V1.

**Изменения в коде:**

* `crates/broker/` (новый crate): `src/main.rs` на axum/actix‑web.

  * `POST /search` — тело запроса по RFC, ответ: страница + cursor.
  * `GET /manifest/:shard` — отдаёт JSON (из локального storage/mock).
* Использовать: `grepzilla_segment::{manifest, manifest_store, cursor}` и reader V1.

**Acceptance:**

* [ ] `curl`/Postman: две страницы подряд, курсор стабилен (pin gen).

---

## EPIC C: Segment V2 (mmap/FST/blocks) — подготовка

### C1 — Абстракция API для reader/writer

**Цель:** отвязать CLI/брокер от конкретной реализации.

**Изменения в коде:**

* `crates/grepzilla_segment/src/api.rs` (новый): trait’ы `SegmentReader/Writer`.
* JSON‑реализацию подключить как один из `impl`.

**Acceptance:**

* [ ] `gzctl` компилируется без изменений.

---

### C2 — RFC формата V2

**Цель:** зафиксировать двоичный формат и оффсеты.

**Изменения:**

* `docs/rfcs/0002-segment-v2-format.md` (новый): файлы `grams.idx/dat`, `docs.dat`, meta, checksum.

**Acceptance:**

* [ ] документ согласован.

---

## EPIC D: Verify Engine

### D1 — Trait и текущая реализация

**Цель:** возможность заменить regex‑библиотеку без правок call‑sites.

**Изменения в коде:**

* `crates/grepzilla_segment/src/verify.rs` (новый): `trait VerifyEngine`, `struct RegexVerify` (на `regex`).
* `crates/gzctl/src/main.rs`: использовать `VerifyEngine` через DI.

**Acceptance:**

* [ ] Поиск работает с новой прослойкой.

---

## EPIC E: Ingest (WAL + memtable + flush)

### E1 — Заготовки WAL/memtable/flusher

**Цель:** иметь L0 свежести до публикации манифеста.

**Изменения в коде:**

* `crates/ingest/` (новый): `wal.rs`, `memtable.rs`, `flusher.rs`.
* Демо: записать документы → видеть их в демо‑брокере (read path: memtable → segments).

**Acceptance:**

* [ ] Ручной тест: новые записи видны до флеша.

---------

Эпик A — сегмент/инвертированный индекс (ingest, нормализация, префильтр на триграммах, verify через regex).

Эпик B — поисковый планировщик (куэрисы, курсорная пагинация, лимиты/offset).

Эпик C — многосегментный поиск (search-coordinator, мёрдж результатов).

Эпик D — распределённость (шардинг, репликация через etcd, согласованность).

Эпик E — DevOps/инфра (CLI, тестовые данные, нагрузочные тесты, CI).