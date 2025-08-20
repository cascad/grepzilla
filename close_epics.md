Sprint 1A — Segment V1 и удобство локального поиска (EPIC A)
A1.1 — Починить workspace и snake_case имена

Цель: проект собирается; gzctl работает.

Файлы и правки:

Cargo.toml (корень):

members = ["crates/grepzilla_segment","crates/gzctl"]

crates/gzctl/Cargo.toml:

заменить зависимость → grepzilla_segment = { path = "../grepzilla_segment" }

все исходники:

use grepzilla-segment::... → use grepzilla_segment::...

Acceptance:

--------

A1.2 — Нормализация и 3-граммы: финальная проверка

Цель: убедиться, что нормализация и разбиение консистентны в writer/reader.

Файлы и правки:

crates/grepzilla_segment/src/normalizer.rs — оставить lower + NFKC + strip accents.

crates/grepzilla_segment/src/gram.rs — trigrams(s: &str) -> Vec<String>.

Acceptance:

---------

A2.1 — Field mask (сбор во время build-seg)

Цель: собирать маски полей для ускорения --field на префильтре.

Файлы и правки:

crates/grepzilla_segment/src/segjson.rs

writer: при обходе полей копить HashMap<String, Vec<u32>> для строковых полей;

по завершении писать field_masks.json в папку сегмента: { "text.body":[0,2,5], "text.title":[0,1,2,...] }.

Acceptance:

----------

A2.2 — Prefilter: пересечение с field mask

Цель: при указании поля пересекать bitmap кандидатов с маской поля до verify.

Файлы и правки:

crates/grepzilla_segment/src/segjson.rs

reader: загружать field_masks.json в HashMap<String, Bitmap>;

fn prefilter(&self, op, grams, /*+opt field: Option<&str>*/) — добавить параметр поля;

если Some(field), делать acc.and_inplace(field_mask).

crates/gzctl/src/main.rs

при --field text.body — передавать поле в prefilter.

Acceptance:

---------------

A3 — Улучшенный preview + подсветка

Цель: показывать короткий сниппет из text.body/text.title с подсветкой первой совпавшей области.

Файлы и правки:

crates/gzctl/src/main.rs

после проверки rx.is_match, найти find() и построить сниппет вида "... <match> ..." (ограничить до ~80 символов);

подсветка простая: заключить совпадение в [...].

Acceptance:

----------

A4 — Мини-метрики поиска

Цель: видеть эффект префильтра/верификации.

Файлы и правки:

crates/gzctl/src/main.rs

добавить флаг --debug-metrics;

печатать в конце запроса: candidates_total, verified_total, hits_total.

Acceptance:

----------

Sprint 1B — Manifest через etcd (mock) и курсоры (EPIC B)
B1 — Типы манифеста/пойнтера и сериализация

Цель: кодовые структуры из RFC.

Файлы и правки:

crates/grepzilla_segment/src/manifest.rs (новый)

pub struct ManifestPtr { epoch: u64, gen: u64, url: String, checksum: String, updated_at: String }

pub struct SegmentMeta { id: String, url: String, min_doc: u32, max_doc: u32, time_min: i64, time_max: i64 }

pub struct TombMeta { cardinality: u64, url: String }

pub struct ManifestV1 { version: u32, shard_id: u64, gen: u64, created_at: String, hwm_seqno: String, segments: Vec<SegmentMeta>, tombstones: TombMeta, prev_gen: Option<u64> }

crates/grepzilla_segment/src/lib.rs: pub mod manifest;

Acceptance:

-----------

B2 — Хранилище указателя (mock impl)

Цель: абстракция для etcd, пока in-memory.

Файлы и правки:

crates/grepzilla_segment/src/manifest_store.rs (новый)

pub trait ManifestStore { fn get_ptr(&self, shard: u64) -> anyhow::Result<ManifestPtr>; fn cas_ptr(&self, shard: u64, expected_gen: u64, new_ptr: &ManifestPtr) -> anyhow::Result<()> }

pub struct InMemoryManifestStore { /*HashMap<u64, ManifestPtr>*/ } + impl

Acceptance:

--------------

B3 — Модель курсора

Цель: сериализуемый курсор с pin-gen.

Файлы и правки:

crates/grepzilla_segment/src/cursor.rs (новый)

pub struct ShardPos { shard: u64, segment: String, block: u32, last_docid: u32 }

pub struct Budgets { candidates: u64, verify_ms: u64 }

pub struct SearchCursor { matcher_hash: String, pin_gen: std::collections::HashMap<u64,u64>, state: Vec<ShardPos>, budgets: Budgets }

helper для matcher_hash (sha256 от нормализованного wildcard+field)

Acceptance:

------------

B4 — Demo-broker (HTTP) с курсорами и pin-gen (локально)

Цель: минимальный HTTP-брокер, который:

читает ManifestPtr (из InMemoryManifestStore);

грузит ManifestV1 (из локальной FS);

выполняет поиск по сегменту(ам) V1;

возвращает стриминг-страницы с SearchCursor.

Файлы и правки:

crates/broker/Cargo.toml (новый)

crates/broker/src/main.rs (новый)

web-фреймворк (axum/actix-web);

POST /search (по RFC, упрощённо wildcard, field, page.size, cursor);

внутренняя логика: нормализация → grams → prefilter (через reader) → verify → сбор hits → cursor;

GET /manifest/:shard — отдаёт JSON из FS (заглушка).

интеграция с grepzilla_segment::{manifest, manifest_store, cursor}.

Acceptance:

-------------

B5 — Режимы консистентности (флаг запроса)

Цель: зафиксировать API: consistency=ONE|QUORUM|ALL (реально пока ONE).

Файлы и правки:

crates/broker/src/main.rs

распарсить флаг consistency;

пока игнорировать (всегда ONE), но эхоить в ответ.

Acceptance:

------------

Итог спринтов

Sprint 1A: быстрый локальный поиск в одном сегменте с field mask, подсветкой и метриками.

Sprint 1B: минимальный брокер с курсорами и pin_gen на мок-манифесте (пролог к etcd).