//! WASI Preview 2 port of `examples/write_file.rs`.
//!
//! Builds a two-group MDF file entirely through the imported `mdf-writer`
//! resource, writes it to `example.mf4` in the preopened directory, then reads
//! it back through the `mdf` reader to verify the group/channel/record counts.

wit_bindgen::generate!({
    world: "mf4-consumer",
    path: "../../wasi/wit",
});

use mf4::core::reader::Mdf;
use mf4::core::types::{DataType, DecodedValue};
use mf4::core::writer::MdfWriter;

fn main() {
    if let Err(err) = run() {
        eprintln!("write_file failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let path = "example.mf4";

    let writer = MdfWriter::new();
    writer.init_mdf_file()?;

    // -------- Channel Group 1 with 2 channels --------
    let cg1 = writer.add_channel_group(Some("Group1"))?;
    writer.add_channel(&cg1, "Speed", DataType::UnsignedIntegerLe)?;
    writer.add_channel(&cg1, "Rpm", DataType::UnsignedIntegerLe)?;

    // -------- Channel Group 2 with 3 channels --------
    let cg2 = writer.add_channel_group(Some("Group2"))?;
    writer.add_channel(&cg2, "Temperature", DataType::SignedIntegerLe)?;
    writer.add_float32_channel(&cg2, "Pressure")?;
    writer.add_channel(&cg2, "Status", DataType::UnsignedIntegerLe)?;

    // -------- Write sample data for both groups --------
    writer.start_data_block(&cg1)?;
    for i in 0u64..100 {
        writer.write_record(
            &cg1,
            &[
                DecodedValue::UnsignedInteger(i),
                DecodedValue::UnsignedInteger(i * 2),
            ],
        )?;
    }
    writer.finish_data_block(&cg1)?;

    writer.start_data_block(&cg2)?;
    for i in 0u64..100 {
        writer.write_record(
            &cg2,
            &[
                DecodedValue::SignedInteger(i as i64 - 50),
                DecodedValue::Float(i as f64 * 0.1),
                DecodedValue::UnsignedInteger(i % 2),
            ],
        )?;
    }
    writer.finish_data_block(&cg2)?;

    writer.finalize_to_file(path)?;
    println!("Wrote {path}");

    // -------- Verify using the reader --------
    let mdf = Mdf::from_file(path)?;
    let groups = mdf.groups()?;
    println!("Channel groups: {}", groups.len());
    for (idx, group) in groups.iter().enumerate() {
        let name = group.name.as_deref().unwrap_or("<unnamed>");
        print!(
            "  Group {} ({}) has {} channels",
            idx + 1,
            name,
            group.channels.len()
        );
        if let Some(first) = group.channels.first() {
            let values = mdf.values(first.name.as_deref().unwrap_or(""), Some(name))?;
            println!(" and {} records", values.len());
        } else {
            println!();
        }
    }

    Ok(())
}
