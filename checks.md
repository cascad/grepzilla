A1
cargo build --release
./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000001
./target/release/gzctl search-seg --seg segments/000001 --q "*играет*" --debug-metrics

A2
cargo build --release
cargo test --workspace

./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000002
./target/release/gzctl search-seg --seg segments/000002 --q "*игра*" --debug-metrics
./target/release/gzctl search-seg --seg segments/000002 --q "*игра*" --field text.body --debug-metrics

A3
cargo build --release
./target/release/gzctl search-seg --seg segments/000002 --q "*игра*" --field text.body
./target/release/gzctl search-seg --seg segments/000002 --q "*играет*"

A4
cargo build --release

# Сегмент из A2 уже есть (segments/000002). Иначе пересобери:
# ./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000002

./target/release/gzctl search-seg --seg segments/000002 --q "*игра*" --field text.body --debug-metrics
./target/release/gzctl search-seg --seg segments/000002 --q "*играет*" --debug-metrics

B1
# все тесты бахнуть
cargo test -p grepzilla_segment
# отдельно
cargo test -p grepzilla_segment manifest
cargo test -p grepzilla_segment manifest_store

B3
cargo test -p grepzilla_segment cursor

-------------

B4

4) Как прогнать

Собрать и запустить:

cargo run -p broker

Сделать сегмент (если ещё не сделали A2):

./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000002

Запрос (с одним сегментом):

curl -s -X POST http://localhost:8080/search \
  -H 'Content-Type: application/json' \
  -d '{
    "wildcard": "*игра*",
    "field": "text.body",
    "segments": ["segments/000002"],
    "page": { "size": 2, "cursor": null }
  }' | jq .

Повтор (следующая страница) — подставь cursor из ответа:

curl -s -X POST http://localhost:8080/search \
  -H 'Content-Type: application/json' \
  -d '{
    "wildcard": "*игра*",
    "field": "text.body",
    "segments": ["segments/000002"],
    "page": { "size": 2, "cursor": { ... } }
  }' | jq .

На этом этапе курсор фиксирует позицию по каждому сегменту (через ShardPos.last_docid). Поле pin_gen есть, но мы его не используем до внедрения настоящих manifest_ptr/manifest.json. Когда перейдём к etcd — брокер начнёт пинить gen и получать состав сегментов по шартам.


cargo run -p broker
# в другом окне:
./target/release/gzctl build-seg --input examples/data.jsonl --out segments/000002
curl -s -X POST http://localhost:8080/search -H 'Content-Type: application/json' -d '{"wildcard":"*игра*","field":"text.body","segments":["segments/000002"],"page":{"size":2,"cursor":null}}' | jq .

--------

# собрать все в релизе
cargo build --release --workspace
# собрать конкретный бинарь
cargo build --release -p gzctl

# запуск
.\target\release\broker.exe
.\target\release\gzctl.exe build-seg --input examples\data.jsonl --out segments\000002

curl -Method POST http://localhost:8080/search `
  -ContentType 'application/json' `
  -Body '{"wildcard":"*игра*","field":"text.body","segments":["segments/000002"],"page":{"size":2,"cursor":null}}'

---------

# Убедимся, что сегмент реально ищется с CLI
# Должны быть хиты и метрики (candidates_total > 0). Если тут пусто — дело в сегменте, а не в брокере.
cargo run -p gzctl -- search-seg --seg segments\000002 --q "*игра*" --field text.body --debug-metrics

---------

B5

# собрать всё
cargo build --release --workspace

# запустить брокер (HTTP /search и /ingest/batch)
cargo run -p broker

--------

# поиск параллельно

curl -s -X POST http://localhost:8080/search \
  -H 'Content-Type: application/json' \
  -d '{
    "wildcard":"*игра*",
    "field":"text.body",
    "segments":["segments/000001","segments/000002","segments/000003"],
    "page":{"size":50,"cursor":null},
    "limits":{"parallelism":4,"deadline_ms":800,"max_candidates":200000}
  }' | jq .


----------

#  Ингест батчем → WAL → сегмент

curl -s -X POST http://localhost:8080/ingest/batch \
-H 'Content-Type: application/json' \
-d '{"records":[{"id":1,"text":{"body":"foo"}},{"id":2,"text":{"body":"игра"}}]}'


Что остаётся “to wire”

Подключить реальную функцию поиска по одному сегменту вместо заглушки storage_adapter::search_one_segment. Там должен быть:
индекс → кандидаты (чанками) → regex-match (первый hit early-return) → учёт max_candidates.

Привинтить HTTP-роутер (axum/actix) к SearchCoordinator::handle и ingest::handle_batch_json.

В executor проставить time_to_first_hit_ms (фиксируем Instant и первый прихода hit).

В reader дописать стрим-итератор с mmap (сейчас — каркас).

Если хочешь, в следующем шаге я:

Вырежу заглушки и подключу твой реальный поиск по сегменту (покажи мне сигнатуру функции, которая сейчас ищет по одному сегменту).

Сразу добавлю роуты на axum и минимальные метрики Prometheus.

--------

B6

8) Логи/диагностика

Добавь (на время) debug! в handle:

какие shards пришли,

что выбрал selected_segments,

какой pin_gen проставился.

Это сэкономит массу времени при первых сменах manifest.json.

Если всё ок — дальше можно параллельно:

добавить FsManifestStore::watch (reload on change),

или подключить etcd-реализацию ManifestStore.

Готов прислать заготовку интеграционного теста search_http_shards.rs с временным manifest.json и двумя сегментами, если хочешь проверить B6 end-to-end.


--------

# из корня репо
cargo build --release -p gzctl

$root = (Resolve-Path ".").Path
.\target\release\gzctl.exe build-seg --input "$root\examples\data.jsonl" --out "$root\segments\000001"
.\target\release\gzctl.exe build-seg --input "$root\examples\data.jsonl" --out "$root\segments\000002"

# быстрая самопроверка сегмента:
.\target\release\gzctl.exe search-seg --seg "$root\segments\000001" --q "*игра*" --field text.body --debug-metrics

--------

1) manifest.json (поддерживаются оба формата)
Вариант A — как у тебя (плоский)
```json
{
  "shards": { "0": 1, "1": 7 },
  "segments": {
    "0:1": ["D:\\\\rust_repo\\\\grepzilla\\\\segments\\\\000001"],
    "1:7": ["D:\\\\rust_repo\\\\grepzilla\\\\segments\\\\000002"]
  }
}
```

Вариант B — V1 (как в тесте)
```json
{
  "version": 1,
  "shards": {
    "0": { "gen": 1, "segments": ["D:\\\\rust_repo\\\\grepzilla\\\\segments\\\\000001"] },
    "1": { "gen": 7, "segments": ["D:\\\\rust_repo\\\\grepzilla\\\\segments\\\\000002"] }
  }
}
```

2) Запуск брокера
# путь к манифесту (можно абсолютный)

$env:GZ_MANIFEST = (Resolve-Path ".\manifest.json").Path
# полезные логи
$env:RUST_LOG = "broker=debug,grepzilla_segment=debug"
cargo run -p broker

В логах на POST увидишь: HIT /search, resolved shards -> segments, pin_gen=...

-----

Запрос через shards (B6)

```sh
# тело запроса (UTF-8 без BOM)
$json = '{"wildcard":"*игра*","field":"text.body","shards":[0,1],
          "page":{"size":2,"cursor":null},
          "limits":{"parallelism":2,"deadline_ms":1000,"max_candidates":200000}}'

$utf8 = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllBytes("req.json", $utf8.GetBytes($json))

# именно настоящий curl.exe
& "$env:SystemRoot\System32\curl.exe" -s -X POST "http://127.0.0.1:8080/search" `
  -H "Content-Type: application/json; charset=utf-8" `
  --data-binary "@req.json"
```

Что проверить в ответе

hits — есть совпадения;
cursor.per_seg — по одному ключу на каждый сегмент;
cursor.pin_gen — должен быть {"0":1,"1":7} (или как в твоём manifest.json).

-----

Следующая страница (подставить cursor из ответа)

```sh
$cursor = '{"per_seg":{"D:\\\\rust_repo\\\\grepzilla\\\\segments\\\\000001":{"last_docid":1},"D:\\\\rust_repo\\\\grepzilla\\\\segments\\\\000002":{"last_docid":1}},"pin_gen":{"0":1,"1":7}}'

$json = '{"wildcard":"*игра*","field":"text.body","shards":[0,1],
          "page":{"size":2,"cursor":'+$cursor+'},
          "limits":{"parallelism":2,"deadline_ms":1000,"max_candidates":200000}}'

[IO.File]::WriteAllBytes("req.json", $utf8.GetBytes($json))

& "$env:SystemRoot\System32\curl.exe" -s -X POST "http://127.0.0.1:8080/search" `
  -H "Content-Type: application/json; charset=utf-8" `
  --data-binary "@req.json"

```

-----

Совместимость: старый режим (segments напрямую)

```sh
$seg1 = (Resolve-Path ".\segments\000001").Path.Replace('\','\\')
$seg2 = (Resolve-Path ".\segments\000002").Path.Replace('\','\\')

$json = '{"wildcard":"*игра*","field":"text.body",
          "segments":["'+$seg1+'","'+$seg2+'"],
          "page":{"size":2,"cursor":null},
          "limits":{"parallelism":2,"deadline_ms":1000,"max_candidates":200000}}'
[IO.File]::WriteAllBytes("req.json", $utf8.GetBytes($json))

& "$env:SystemRoot\System32\curl.exe" -s -X POST "http://127.0.0.1:8080/search" `
  -H "Content-Type: application/json; charset=utf-8" `
  --data-binary "@req.json"
```

-----

Что дальше по твоему roadmap (после полного B4)

На выбор — оба пути валидны:

EPIC C2 (RFC V2 формата сегмента) — документация, можно параллелить.

EPIC D1 (Verify Engine trait) — у тебя уже есть зачаток; вынести в отдельный модуль и «внедрить» через DI, чтобы можно было менять движок верификации.

EPIC E1 (Ingest: WAL + memtable + flush) — даст UX: не собирать сегменты руками. Минимум:

POST /ingest/batch_json (мы уже тест готовили ранее).

WAL append → периодический build-seg → обновить manifest (gen++).

Интеграционный тест «записал → увидел в поиске».

Если хочешь — выбери, с чего стартуем, и я дам конкретный diff/скелет под выбранный эпик.


--------
C3

cargo test -p grepzilla_segment v2_prefilter_and_field_mask_roundtrip -- --nocapture

Дальше (план работ C3 — короткие итерации)

Writer — grams:
собрать 3-граммы → отсортировать ключи → записать grams.idx/dat (сначала inline для маленьких списков).
Проверка: утилита-дампер показывает корректные offsets/length, varint-раскодировка даёт те же doc_id, что V1.

Writer — fields:
для каждого field_name построить Roaring, порог tiny_set → записать fields.idx/dat.
Проверка: пересечение с префильтром даёт одинаковые candidates_total с V1.

Writer — docs:
упаковать поля по блокам, CRC32 per block.
Проверка: быстрый get_doc возвращает те же строки, что V1.

Reader — prefilter():
бинарный поиск по grams.idx, итерация grams.dat, пересечение с fields.
Проверка: gzctl search-seg (V2) == V1 по hits.

Reader — get_doc():
навигация по docs.dat блокам, извлечение полей.
Проверка: сниппеты/preview такие же, как V1.

-----

```sh
cargo test -p grepzilla_segment v2_docs_roundtrip
cargo test -p grepzilla_segment v2_prefilter_then_get_doc
```

Новые тесты проверяют:

docs.dat round-trip: запись → чтение get_doc(), соответствие doc_id, ext_id, наличие ожидаемых полей/значений.

Порча CRC у docs.dat → open_segment() падает.

Интеграция: prefilter(AND, ["мир"], Some("text.body")) находит нужный doc_id, а get_doc() возвращает корректный документ.

--------

Предлагаю дорожку:

C3.3 – интеграция get_doc()

Вынести авто-детект V1/V2 в фабрику open_segment(path) — ты уже делаешь в SegmentReader.

В CLI search-seg:

если сегмент V2 → делать prefilter, а затем брать документы через get_doc();

собрать JSON-ответ с полями _id + несколько строковых полей (как в V1).

В broker: заменить заглушку на вызов get_doc() и выдачу превью.

C3.4 – нормализация превью
Решить: сколько полей/символов в превью, чтобы не тащить весь документ.

C3.5 – кэширование / оптимизация
Подумать: держать ли горячие документы (LruCache) поверх OnceCell, или OnceCell уже достаточно.

---------

C3.3 → C3.9: статус

✅ C3.3 — docs.dat (writer/reader)

Реализовано: компактный формат с оффсет-таблицей, CRC64; чтение по оффсетам.

Код: grepzilla_segment/v2/writer.rs (docs.dat запись), v2/reader.rs (парсинг).

✅ C3.4 — get_doc() + минимальный кеш + prefetch

Реализовано: get_doc(&self, doc_id) -> Option<&StoredDoc> с OnceCell<Vec>; prefetch_docs<I: IntoIterator<u32>>().

Код: v2/reader.rs (кеш + prefetch).

✅ C3.5 — gzctl: авто-детект V2 и поиск

Реализовано: gzctl search-seg работает и с V1, и с V2; мультисегмент через список директорий тоже есть.

Код: gzctl/src/main.rs.

✅ C3.6 — broker: V2 превью + shards/manifest

Реализовано: /search через shards → manifest.resolve; V2 ветка использует prefetch_docs() + get_doc(); превью с подсветкой матча.

Код: broker/http_api.rs (у тебя было), broker/search/mod.rs (координатор), broker/storage_adapter.rs.

✅ C3.7 — улучшения merge/pagination

Реализовано: дедуп по ext_id при merge, приоритет свежих генов (сортировка по gen DESC внутри шарда).

Код: broker/search/paginator.rs (дедуп), broker/search/mod.rs (сортировка выбранных сегов).

✅ C3.8 — метрики и прогрев

Реализовано: prefetch = page.size * 4 (cap 5000) для V2; добавлена метрика dedup_dropped.

Код: broker/storage_adapter.rs (prefetch), broker/search/types.rs (метрика), broker/search/mod.rs (прокидка).

✅ C3.9 — мультисегментный поиск

Реализовано: через shards/manifest (продовый путь) и через явные segments (debug). Курсор per_seg и pin_gen в ответе.

--------

# 1) Запишем req.json (UTF-8 без BOM)
$json = '{
  "wildcard":"*игра*",
  "field":"text.body",
  "shards":[0,1],
  "page":{"size":5,"cursor":null},
  "limits":{"parallelism":2,"deadline_ms":1000,"max_candidates":200000}
}'
$utf8 = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllBytes("req.json", $utf8.GetBytes($json))

# 2) Отправляем настоящим curl.exe
& "$env:SystemRoot\System32\curl.exe" -s -X POST "http://127.0.0.1:8080/search" `
  -H "Content-Type: application/json; charset=utf-8" `
  --data-binary "@req.json" | ConvertFrom-Json | ConvertTo-Json -Depth 8

дедуп работает: один ext_id:"2"; dedup_dropped: 1 — один дубль выкинули;
превью из поля матча с подсветкой: "[щенок играет с мячиком]";
курсор есть по обоим сегам + pin_gen подтянулся из манифеста.

------

Быстрый полный прогон

```sh
# зачистка артефактов
cargo clean

# линтеры (полезно поймать «дрожащие» импорты)
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings

# прогнать конкретный
cargo test -p broker --test search_shards_v2 -- --nocapture

# юниты/интеграционные по всему дереву
cargo test --workspace --all-targets -- --nocapture
```

------------

Отлично 🎉 — тесты зелёные, значит блок C3.x (docs.dat, get_doc, prefetch, мультисегмент через shards, dedup, preview) мы довели до конца.

📌 Если смотреть на roadmap, у нас:

EPIC A (V1) — закрыт.

EPIC B (Manifest + курсоры) — закрыт, ты даже тестишь через shards.

EPIC C (V2) — до C3.9 включительно сделали: полноценный поиск V2, интеграция с брокером, курсоры, превью, дедуп, тесты/бенчи.

EPIC D (Verify Engine) — ещё не трогали (там про абстракцию verify движка, возможность заменить regex).

EPIC E (Ingest/WAL/memtable) — тоже не трогали.

Варианты, куда двигаться дальше

EPIC D. Verify Engine

Вынести regex-проверку в отдельный trait (VerifyEngine).

Подготовить точку расширения, чтобы потом легко заменить на Hyperscan, Rust-regex-automata, PCRE2.

Минимальный шаг: RegexVerify реализует этот trait, а брокер и CLI используют его через DI.

Тесты: подменить VerifyEngine на "заглушку" (всегда true/false).

EPIC E. Ingest

WAL + memtable, свежие документы до флеша.

Тяжелее и больше кода, но даст live-запись.

Оптимизация текущего (C4)

Вынести блок-кодеки для postings (сейчас только kind=1 inline varints).

Добавить block codec (kind=2) с компрессией пачек doc_id.

Влияет на производительность и размер.

Инфра

CI, нагрузочные бенчи, замеры latency/throughput.

👉 Логично пойти в EPIC D (Verify Engine) — это меньше по объёму, но сразу даст гибкость и подготовит к перформанс-тюнингу.

Хочешь, я распишу D1/D2 как карточки задач (по аналогии с A/B/C), и мы начнём с вынесения VerifyEngine в grepzilla_segment?


-----------

Варианты, куда двигаться дальше

EPIC D. Verify Engine

Вынести regex-проверку в отдельный trait (VerifyEngine).

Подготовить точку расширения, чтобы потом легко заменить на Hyperscan, Rust-regex-automata, PCRE2.

Минимальный шаг: RegexVerify реализует этот trait, а брокер и CLI используют его через DI.

Тесты: подменить VerifyEngine на "заглушку" (всегда true/false).

EPIC E. Ingest

WAL + memtable, свежие документы до флеша.

Тяжелее и больше кода, но даст live-запись.

Оптимизация текущего (C4)

Вынести блок-кодеки для postings (сейчас только kind=1 inline varints).

Добавить block codec (kind=2) с компрессией пачек doc_id.

Влияет на производительность и размер.

Инфра

CI, нагрузочные бенчи, замеры latency/throughput.


-------------

D1

# дефолтный движок (regex)
cargo fmt --all
cargo check --workspace
cargo test --workspace --all-targets -- --nocapture

# включаем pcre2-движок
cargo test --workspace --all-targets --features engine-pcre2 -- --nocapture

# брокер локально
GZ_VERIFY=pcre2 cargo run -p broker
GZ_VERIFY=pcre2 cargo run -p broker --features engine-pcre2
# или с regex (по умолчанию):
cargo run -p broker

--------

# 📌 Эпик D — Продвинутый поиск и метрики

## 🔹 D4. Dedup
- [ ] **D4.1** Добавить счётчик `dedup_dropped` в `Paginator::merge`.
- [ ] **D4.2** Написать тест: два сегмента с одинаковым `ext_id` → должен остаться один hit.
- [ ] **D4.3** Проверить метрику `dedup_dropped` в ответе.

## 🔹 D5. Cursor & Pagination
- [ ] **D5.1** Доработать `Paginator::merge`, чтобы курсор хранил `last_docid` на сегмент.
- [ ] **D5.2** Добавить обработку `page.cursor` в `SearchCoordinator`, чтобы продолжать поиск с указанного docid.
- [ ] **D5.3** Написать тесты:
  - первая страница,
  - вторая страница с курсором (hits разные, без повторов).

## 🔹 D6. Metrics расширенные
- [ ] **D6.1** Проверить, что `prefilter_ms`, `verify_ms`, `prefetch_ms`, `warmed_docs` агрегируются в `SearchResponse`.
- [ ] **D6.2** Добавить тест: синтетическая нагрузка → метрики должны быть >0.
- [ ] **D6.3** Убедиться, что метрики сериализуются как `null`, если пустые.

## 🔹 D7. API/UX
- [ ] **D7.1** Проверить стабильность JSON-ответов (SearchResponse, ManifestShardOut).
- [ ] **D7.2** Добавить эндпоинт `GET /healthz` (возвращает `{status:"ok"}`).
- [ ] **D7.3** Написать e2e-тесты для `GET /healthz` и `GET /manifest/:shard`.

## 🔹 D8. Документация
- [ ] **D8.1** Обновить README: примеры `POST /search` (shards + segments).
- [ ] **D8.2** Описать `GZ_MANIFEST` и `GZ_VERIFY` (regex / pcre2).
- [ ] **D8.3** Добавить примеры PowerShell-запросов (для Windows).

## 🔹 D9. Cleanup
- [ ] **D9.1** Удалить остаточные `use regex::Regex` (кроме verify_impl).
- [ ] **D9.2** Удалить старые/устаревшие тесты (например, для V1 без манифеста).
- [ ] **D9.3** Финальный прогон `cargo test --workspace` и `cargo clippy --all-targets`.
