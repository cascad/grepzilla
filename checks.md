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
