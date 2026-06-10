//! Merge throughput benchmarks: import-chain fusing across many modules, and
//! re-encoding of large function bodies.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use wasm_fuse::{ExportConflictPolicy, MergeOptions, Merger};

/// A chain of `length` modules: each calls the next module's export.
fn chain_modules(length: usize) -> Vec<(String, Vec<u8>)> {
    (0..length)
        .map(|index| {
            let source = if index == length - 1 {
                "(module (func (export \"f\") (result i32) (i32.const 0)))".to_string()
            } else {
                format!(
                    "(module
                        (import \"module{}\" \"f\" (func $next (result i32)))
                        (func (export \"f\") (result i32) (call $next)))",
                    index + 1
                )
            };
            (format!("module{index}"), wat::parse_str(source).unwrap())
        })
        .collect()
}

/// Two modules with `functions` small functions each, every one exported.
fn wide_modules(functions: usize) -> Vec<(String, Vec<u8>)> {
    (0..2)
        .map(|module| {
            let mut source = String::from("(module ");
            for function in 0..functions {
                source.push_str(&format!(
                    "(func (export \"f{function}\") (result i32) \
                     (i32.add (i32.const {function}) (i32.const 1)))"
                ));
            }
            source.push(')');
            (format!("module{module}"), wat::parse_str(source).unwrap())
        })
        .collect()
}

fn merge(modules: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut merger = Merger::new(MergeOptions {
        export_conflicts: ExportConflictPolicy::Rename,
        ..MergeOptions::default()
    });
    for (name, binary) in modules {
        merger.add_module(name.clone(), binary).unwrap();
    }
    merger.merge().unwrap()
}

fn bench_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("chain");
    for length in [2usize, 8, 32] {
        let modules = chain_modules(length);
        group.bench_with_input(
            BenchmarkId::from_parameter(length),
            &modules,
            |bencher, modules| bencher.iter(|| merge(black_box(modules))),
        );
    }
    group.finish();

    let mut group = c.benchmark_group("wide");
    for functions in [100usize, 1000] {
        let modules = wide_modules(functions);
        group.bench_with_input(
            BenchmarkId::from_parameter(functions),
            &modules,
            |bencher, modules| bencher.iter(|| merge(black_box(modules))),
        );
    }
    group.finish();
}

criterion_group!(benches, bench_merge);
criterion_main!(benches);
