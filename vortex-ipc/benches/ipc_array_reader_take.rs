use std::io::Cursor;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use fallible_iterator::FallibleIterator;
use itertools::Itertools;
use vortex::array::primitive::PrimitiveArray;
use vortex::{Context, IntoArray};
use vortex_dtype::{DType, Nullability, PType};
use vortex_ipc::iter::FallibleLendingIterator;
use vortex_ipc::reader::StreamReader;
use vortex_ipc::writer::StreamWriter;

// 100 record batches, 100k rows each
// take from the first 20 batches and last batch
// compare with arrow
fn ipc_array_reader_take(c: &mut Criterion) {
    let ctx = Context::default();

    let indices = (0..20)
        .map(|i| i * 100_000 + 1)
        .chain([98 * 100_000 + 1])
        .collect_vec();
    let mut group = c.benchmark_group("ipc_array_reader_take");

    group.bench_function("vortex", |b| {
        let mut buffer = vec![];
        {
            let mut cursor = Cursor::new(&mut buffer);
            let mut writer = StreamWriter::try_new(&mut cursor, &ctx).unwrap();
            writer
                .write_schema(&DType::Primitive(PType::I32, Nullability::Nullable))
                .unwrap();
            (0..100i32).for_each(|i| {
                let data = PrimitiveArray::from(vec![i; 100_000]).into_array();
                writer.write_batch(&data).unwrap();
            });
        }
        let indices = indices.clone().into_array();

        b.iter(|| {
            let mut cursor = Cursor::new(&buffer);
            let mut reader = StreamReader::try_new(&mut cursor, &ctx).unwrap();
            let array_reader = reader.next().unwrap().unwrap();
            let mut iterator = array_reader.take(&indices).unwrap();
            while let Some(arr) = iterator.next().unwrap() {
                black_box(arr);
            }
        });
    });
}

criterion_group!(benches, ipc_array_reader_take);
criterion_main!(benches);
