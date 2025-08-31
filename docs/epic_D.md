D1 — Абстракция VerifyEngine (интерфейс + базовая реализация)
Контекст

Сейчас верификация совпадения (после префильтра) захардкожена на regex. Нужно отвязать call-sites от конкретной библиотеки, чтобы можно было подменять движок без правок брокера/CLI.

Цель

Единый trait VerifyEngine + дефолтная реализация RegexVerify на regex, покрытая тестами.

Изменения в коде (пути файлов обязательно!)

crates/grepzilla_segment/src/verify/mod.rs (новый):

pub trait VerifyEngine { fn compile(pattern: &str) -> Result<Self> where Self:Sized; fn is_match(&self, text: &str) -> bool; fn highlight<'a>(&self, text: &'a str) -> Option<(usize,usize)>; }

pub enum PatternKind { Wildcard /*…*/, Regex /*…*/ } (на будущее).

crates/grepzilla_segment/src/verify/regex_impl.rs (новый):

pub struct RegexVerify { rx: regex::Regex }

реализация VerifyEngine (+ режим (?si) по умолчанию).

Заменить прямые вызовы regex в:

crates/broker/src/storage_adapter.rs — через VerifyEngine (DI).

crates/gzctl/src/main.rs — через VerifyEngine.

Обновить helper wildcard_to_regex_case_insensitive → в verify::compile_wildcard().

API / CLI (если есть)

Никаких новых флагов. Внутренний DI: let eng = RegexVerify::compile(&wildcard_to_regex(...))?;

Acceptance Criteria

--------

D2 — Инъекция VerifyEngine в broker + CLI, выбор по флагу env
Контекст

Нужно уметь подменять движок без перекомпиляции всего кода — хотя бы через ENV/конфиг.

Цель

Фабрика VerifyEngine в брокере и CLI: выбор regex по умолчанию, опционально другой engine по GZ_VERIFY_ENGINE=regex|hs|ra.

Изменения в коде

crates/broker/src/search/mod.rs: добавить VerifyFactory в SearchCoordinator или прокинуть через AppState.

crates/broker/src/storage_adapter.rs: получать engine_factory: Arc<dyn VerifyFactory> из AppState.

crates/gzctl/src/main.rs: локально дергать RegexVerify или через same factory.

crates/grepzilla_segment/src/verify/factory.rs (новый): pub trait VerifyFactory { fn compile(&self, wildcard: &str) -> Result<Box<dyn VerifyEngine + Send + Sync>>; }, EnvVerifyFactory.

API / CLI

ENV: GZ_VERIFY_ENGINE=regex (дефолт, backward-compatible).

Acceptance Criteria

-----------

D3 — Нормализация и сопоставление: единая семантика для Verify
Контекст

Мы нормализуем текст (lowercase/trim) для индексации. В verify должны применять совместимую нормализацию и корректно подсвечивать оригинал.

Цель

Единый helper: компиляция паттерна из wildcard с семантикой (?si), сопоставление по нормализованному тексту, подсветка на исходном тексте (с маппингом).

Изменения в коде

crates/grepzilla_segment/src/common/preview.rs: уже UTF-8 safe; использовать verify::highlight если доступно.

crates/grepzilla_segment/src/normalizer.rs: экспорт функции/типов для verify.

crates/grepzilla_segment/src/verify/mod.rs: опциональный метод highlight_normalized(text_raw, text_norm) по умолчанию прокидывает в highlight(text_raw).

Acceptance

---------------

D4 — Hyperscan backend (опционально, feature hs)
Контекст

Для высоких RPS/низкой латентности нужен backend с SIMD/DFAs (Hyperscan).

Цель

Реализация HsVerify за флагом cfg(feature="hs"). Выбор через GZ_VERIFY_ENGINE=hs.

Изменения в коде

grepzilla_segment/Cargo.toml:

[features] hs = ["hyperscan"]

hyperscan = { version = "...", optional = true }

crates/grepzilla_segment/src/verify/hs_impl.rs (новый):

struct HsVerify { db: hyperscan::BlockDatabase, scratch: ... }

Компиляция из regex-паттерна (из wildcard).

verify::factory регистрирует "hs" если feature включена.

Acceptance

----------

D5 — rust-regex-automata backend (ra) (опционально, feature ra)
Контекст

Нативный Rust backend с DFA/ PikeVM; может быть быстрее regex на некоторых классах паттернов.

Цель

Реализация RaVerify за cfg(feature="ra"), выбор GZ_VERIFY_ENGINE=ra.

Изменения

grepzilla_segment/Cargo.toml:

[features] ra = ["regex-automata"]

regex-automata = { version = "0.4", optional = true }

crates/grepzilla_segment/src/verify/ra_impl.rs:

компиляция dfa::regex::Regex с (?si) семантикой, is_match, find.

Acceptance

------------

D6 — Метрики и профилирование Verify
Контекст

Нужно понимать, где горим: compile time, match time, hit rate.

Цель

Собрать простые метрики на уровне storage_adapter и вывести их в ответ, когда включён debug-режим.

Изменения

crates/broker/src/storage_adapter.rs:

засечь время compile, prefilter, verify_passes, preview_time.

суммировать по сегменту и вернуть в SegmentTaskOutput.

crates/broker/src/search/paginator.rs:

агрегировать метрики в SearchMetrics { verify_compile_ms, verify_match_ms, ... } (добавить новые поля).

crates/broker/src/search/types.rs: расширить SearchMetrics (не ломающий Ext API: новые поля опциональны/с дефолтами).

Acceptance

------------

Бонус (малый): D7 — Снимок профиля hot-спотов (bench + flamegraph)
Контекст

Хотим измерить эффекты движков на типичных паттернах.

Цель

criterion-бенчи для verify: компиляция и match на наборах corpora.

Изменения

crates/grepzilla_segment/benches/verify_bench.rs:

cases: короткие/длинные wildcard, кириллица, ascii, много текстов.

Acceptance

----------

Итоговая последовательность

D1: trait + RegexVerify + интеграция → зелёные тесты.

D2: фабрика + ENV выбор → e2e зелёные.

D3: единая семантика нормализации/подсветки → стабильно.

D4/D5: экспериментальные движки за фичами.

D6/D7: метрики и бенчи.