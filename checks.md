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
