use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use discord_bot::utils::ascii::{
    ascii_contains_icase, ascii_starts_with_icase, cmp_ignore_ascii_case, collect_prefix_icase,
};
use std::hint::black_box;

fn bench_cmp_ignore_ascii_case(c: &mut Criterion) {
    let mut group = c.benchmark_group("cmp_ignore_ascii_case");

    let cases = [
        ("hello", "hello"),
        ("Hello", "hello"),
        ("apple", "banana"),
        (&"Z".repeat(100), &"z".repeat(100)),
    ];

    for &(a, b) in &cases {
        let id = format!("\"{a}\" vs \"{b}\"");
        group.bench_with_input(
            BenchmarkId::new("cmp_ignore_ascii_case", id),
            &(a, b),
            |bencher, &(a, b)| bencher.iter(|| cmp_ignore_ascii_case(black_box(a), black_box(b))),
        );
    }

    group.finish();
}

fn bench_ascii_contains_icase(c: &mut Criterion) {
    let mut group = c.benchmark_group("ascii_contains_icase");

    for &size in &[16usize, 64, 256, 1024] {
        let hay = "z".repeat(size) + "hello";

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &hay,
            |b, hay| b.iter(|| ascii_contains_icase(black_box(hay), black_box("hello"))),
        );
    }

    group.finish();
}

fn bench_ascii_starts_with_icase(c: &mut Criterion) {
    let mut group = c.benchmark_group("ascii_starts_with_icase");

    let test_cases = ["HelloWorld", "helloworld", "HELLOWORLD"];

    for &hay in &test_cases {
        group.bench_with_input(
            BenchmarkId::from_parameter(hay),
            &hay,
            |b, &hay| b.iter(|| ascii_starts_with_icase(black_box(hay), black_box("hello"))),
        );
    }

    group.finish();
}

fn bench_collect_prefix_icase(c: &mut Criterion) {
    let mut group = c.benchmark_group("collect_prefix_icase");

    let data: Vec<String> = (0..10_000)
        .map(|i| format!("str{i:05}"))
        .collect();

    let cases =
        [("empty", ""), ("no_match", "zzz"), ("one_match", "str00000"), ("many_match", "str00")];

    for &(name, prefix) in &cases {
        group.bench_with_input(
            BenchmarkId::new("collect_prefix_icase", name),
            &prefix,
            |b, &prefix| {
                b.iter(|| {
                    let out = collect_prefix_icase(
                        black_box(&data),
                        black_box(prefix),
                        |s: &String| s.as_str(),
                    );
                    black_box(out);
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_cmp_ignore_ascii_case,
    bench_ascii_contains_icase,
    bench_ascii_starts_with_icase,
    bench_collect_prefix_icase
);
criterion_main!(benches);
