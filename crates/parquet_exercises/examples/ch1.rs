use std::sync::Arc;

use arrow::{
    array::{Float64Array, Int64Array, ListArray, ListBuilder, PrimitiveBuilder, RecordBatch},
    datatypes::{DataType, Field, Float64Type, Schema},
};
use rand::RngExt;

#[derive(Debug, Clone, PartialEq)]
struct Row {
    id: i64,
    cost: f64,
    cost_components: Vec<f64>,
}

pub fn main() {
    const N: usize = 10;
    let mut rng = rand::rng();
    let rows: Vec<Row> = (0..N)
        .map(|i| Row {
            id: i as i64,
            cost: rng.random_range(0.0..100.0),
            cost_components: (0..rng.random_range(0..10))
                .map(|_| rng.random_range(0.0..1000.0))
                .collect(),
        })
        .collect();

    let original_rows = rows.clone();

    let batch = vec_rows_to_record_batch(rows);
    let reconstructed = record_batch_to_vec_rows(batch);

    dbg!(&original_rows);
    dbg!(&reconstructed);

    assert_eq!(original_rows, reconstructed);
}

fn vec_rows_to_record_batch(rows: Vec<Row>) -> RecordBatch {
    let mut ids = Vec::with_capacity(rows.len());
    let mut costs = Vec::with_capacity(rows.len());
    let mut cost_components_builder = ListBuilder::new(PrimitiveBuilder::<Float64Type>::new())
        .with_field(Arc::new(Field::new("item", DataType::Float64, false)));

    for r in rows {
        ids.push(r.id);
        costs.push(r.cost);

        cost_components_builder
            .values()
            .append_slice(&r.cost_components);
        cost_components_builder.append(true);
    }

    let ids = Arc::new(Int64Array::from(ids));
    let costs = Arc::new(Float64Array::from(costs));
    let cost_components = Arc::new(cost_components_builder.finish());

    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("cost", DataType::Float64, false),
        Field::new(
            "id",
            DataType::List(Arc::new(Field::new("item", DataType::Float64, false))),
            false,
        ),
    ]));

    RecordBatch::try_new(schema, vec![ids, costs, cost_components]).expect("failed to build batch")
}

fn record_batch_to_vec_rows(batch: RecordBatch) -> Vec<Row> {
    let ids = batch
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("failed to downcast id");

    let costs = batch
        .column(1)
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("failed to downcast costs");

    let cost_components = batch
        .column(2)
        .as_any()
        .downcast_ref::<ListArray>()
        .expect("failed to downcast cost_components");

    let cost_component_values = cost_components
        .values()
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("failed to downcast cost_components items");

    let mut reconstructed = Vec::new();

    for i in 0..batch.num_rows() {
        let id = ids.value(i);
        let cost = costs.value(i);

        let start = cost_components.value_offsets()[i] as usize;
        let end = cost_components.value_offsets()[i + 1] as usize;
        let components = (start..end)
            .map(|j| cost_component_values.value(j))
            .collect();

        reconstructed.push(Row {
            id,
            cost,
            cost_components: components,
        });
    }

    reconstructed
}
