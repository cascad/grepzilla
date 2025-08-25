// broker/benches/search_bench.rs
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_search(c: &mut Criterion) {
    c.bench_function("search_parallel_50hits", |b| {
        b.iter(|| {
            // запусти координацию поиска с 5-10 сегментами (моки/фейки)
        });
    });
}

criterion_group!(benches, bench_search);
criterion_main!(benches);
