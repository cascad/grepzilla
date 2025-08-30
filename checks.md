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