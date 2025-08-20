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