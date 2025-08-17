# grepzilla

Minimal in-memory text search engine (Rust) using 3-gram + Roaring bitmaps.

## Build

```bash
cargo build --release

# Example dataset
echo '{"_id":"1","text.title":"Кошки","text.body":"котёнок играет с клубком","tags":["pets"],"lang":"ru"}' > data.jsonl
echo '{"_id":"2","text.title":"Собаки","text.body":"щенок играет с мячиком","tags":["pets"],"lang":"ru"}' >> data.jsonl

# Ingest and run an interactive search
./target/release/textsearch ingest data.jsonl --repl

# Anywhere substring
query> *кот*клуб*

# Field-scoped
query> text.body:*клуб*

# Boolean
query> text.body:*играет* AND ( *коте* OR *щено* )

# Pagination
query> *играет* --limit 1 --offset 1

## License  
Apache 2.0 — свободное использование с сохранением авторских прав.  
Подробнее: [LICENSE](LICENSE).

Notes

Patterns must contain at least one literal run of ≥3 chars after normalization; otherwise they are rejected to avoid full scans.

Wildcards: * any-length, ? single char.

This is the in-memory MVP. Next steps: segments, FST lexicon, mmap, tombstones, merges.


Next Steps (Roadmap inside code comments)

Replace HashMap grams with sorted lexicon + DF stats; pick rare-first intersection order.

Introduce Segment files: FST lexicon, Roaring postings blocks (zstd), doc store; mmap for queries.

Field bitmaps and metadata columns for field: and filters.

Proper boolean parser (nom), parentheses, precedence.

Concurrency: per-segment workers; parallel bitmap ops; top-K heap.

Persistence: snapshot/save & load; WAL; merges; tombstones.

Ranking: BM25F with optional positions.

Bench: microbench for intersections; end-to-end p50/p95/p99 on synthetic mixes.