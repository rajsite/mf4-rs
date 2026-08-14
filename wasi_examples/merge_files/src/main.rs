//! WASI Preview 2 port of `examples/merge_files.rs`.
//!
//! Writes two files with the same channel layout covering consecutive time
//! ranges, merges them with the imported `ops.merge-file`, then reads the
//! merged file back and reports its group count and record totals.

wit_bindgen::generate!({
    world: "mf4-consumer",
    path: "../../wasi/wit",
});

use mf4::core::ops;
use mf4::core::reader::Mdf;
use mf4::core::types::{DataType, DecodedValue};
use mf4::core::writer::MdfWriter;

fn main() {
    if let Err(err) = run() {
        eprintln!("merge_files failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let input1 = "merge_input1.mf4";
    let input2 = "merge_input2.mf4";
    let output = "merge_result.mf4";

    write_segment(input1, 0)?;
    write_segment(input2, 5)?;

    // Merge the two files.
    ops::merge_file(output, input1, input2)?;

    // Inspect using the reader API.
    let mdf = Mdf::from_file(output)?;
    let groups = mdf.groups()?;
    println!("Merged file has {} channel group(s)", groups.len());
    for (idx, group) in groups.iter().enumerate() {
        println!(
            "  Group {}: {} channels, {} records",
            idx + 1,
            group.channels.len(),
            group.record_count
        );
    }
    println!("Created {output}");
    Ok(())
}

/// Write a file with a `Time`/`Value` group covering records `[start, start+5)`.
fn write_segment(path: &str, start: u64) -> Result<(), String> {
    let writer = MdfWriter::new();
    writer.init_mdf_file()?;
    let cg = writer.add_channel_group(None)?;
    writer.add_time_channel(&cg, "Time")?;
    writer.add_channel(&cg, "Value", DataType::UnsignedIntegerLe)?;
    writer.start_data_block(&cg)?;
    for i in start..start + 5 {
        writer.write_record(
            &cg,
            &[
                DecodedValue::Float(i as f64 * 0.1),
                DecodedValue::UnsignedInteger(i),
            ],
        )?;
    }
    writer.finish_data_block(&cg)?;
    writer.finalize_to_file(path)?;
    Ok(())
}
