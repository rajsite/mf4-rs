//! WASI Preview 2 port of `examples/index_operations.rs`.
//!
//! Builds an MDF file through the `mdf-writer`, then exercises the `mdf-index`
//! resource: build an index from the file, serialize it to JSON and reload it,
//! inspect metadata by name, compute byte ranges, and read channel values by
//! binding the index to the file (`values-from-file`).

wit_bindgen::generate!({
    world: "mf4-consumer",
    path: "../../wasi/wit",
});

use mf4::core::index::MdfIndex;
use mf4::core::types::{DataType, DecodedValue};
use mf4::core::writer::MdfWriter;

fn main() {
    if let Err(err) = run() {
        eprintln!("index_operations failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mdf_file = "index_example.mf4";
    let index_file = "index_example.json";

    println!("=== Index Operations Example ===");

    println!("Creating MDF file with test data...");
    create_test_mdf_file(mdf_file)?;

    println!("Creating index and saving to '{index_file}'...");
    let index = MdfIndex::from_file(mdf_file)?;
    index.save_to_file(index_file)?;

    println!("Loading index and reading self-contained metadata...");
    let loaded = MdfIndex::load_from_file(index_file)?;

    println!("\nAvailable channels:");
    for name in loaded.channel_names() {
        println!("  {name}");
    }

    println!("\nChannel values read from the index (Time):");
    let values = loaded.values_from_file("Time", None, mdf_file)?;
    for (i, value) in values.iter().enumerate().take(10) {
        println!("  Record {i}: {value:?}");
    }
    if values.len() > 10 {
        println!("  ... and {} more records", values.len() - 10);
    }

    println!("\nByte Range Analysis (partial reading):");
    let ranges = loaded.byte_ranges("Time", None)?;
    let total: u64 = ranges.iter().map(|r| r.length).sum();
    println!("  Total bytes needed: {total}");
    println!("  Number of byte ranges: {}", ranges.len());
    for r in &ranges {
        println!("    [offset {}, length {}]", r.offset, r.length);
    }

    println!("\nReading 'Temperature' by name:");
    let temp = loaded.values_from_file("Temperature", None, mdf_file)?;
    println!("  Read {} values for channel 'Temperature'", temp.len());

    println!("\nGroups:");
    for group in loaded.groups()? {
        println!(
            "  {} ({} records, master {:?})",
            group.name.as_deref().unwrap_or("<unnamed>"),
            group.record_count,
            group.master_channel
        );
    }

    println!("\nIndex example completed successfully!");
    Ok(())
}

fn create_test_mdf_file(path: &str) -> Result<(), String> {
    let writer = MdfWriter::new();
    writer.init_mdf_file()?;

    let cg = writer.add_channel_group(Some("Measurements"))?;
    writer.add_time_channel(&cg, "Time")?;
    writer.add_channel(&cg, "Temperature", DataType::SignedIntegerLe)?;
    writer.add_float_channel(&cg, "Pressure")?;

    writer.start_data_block(&cg)?;
    for i in 0u64..50 {
        writer.write_record(
            &cg,
            &[
                DecodedValue::Float(i as f64 * 0.01),
                DecodedValue::SignedInteger(i as i64 - 25),
                DecodedValue::Float(1000.0 + i as f64),
            ],
        )?;
    }
    writer.finish_data_block(&cg)?;
    writer.finalize_to_file(path)?;
    Ok(())
}
