//! WASI Preview 2 port of `examples/cut_file.rs`.
//!
//! Writes a small time-series file, cuts the `[0.3, 0.6]` second window with
//! the imported `ops.cut-by-time-file`, then reads the trimmed file back and
//! prints its channel names.

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
        eprintln!("cut_file failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let input = "cut_example_input.mf4";
    let output = "cut_example_output.mf4";

    // Create a simple MF4 file with a time channel and a value channel.
    let writer = MdfWriter::new();
    writer.init_mdf_file()?;
    let cg = writer.add_channel_group(None)?;
    writer.add_time_channel(&cg, "Time")?;
    writer.add_channel(&cg, "Val", DataType::UnsignedIntegerLe)?;
    writer.start_data_block(&cg)?;
    for i in 0u64..10 {
        writer.write_record(
            &cg,
            &[
                DecodedValue::Float(i as f64 * 0.1),
                DecodedValue::UnsignedInteger(i),
            ],
        )?;
    }
    writer.finish_data_block(&cg)?;
    writer.finalize_to_file(input)?;

    // Cut between 0.3 and 0.6 seconds.
    ops::cut_by_time_file(input, output, 0.3, 0.6)?;

    // Inspect the cut file and print channel names.
    let mdf = Mdf::from_file(output)?;
    for (g_idx, group) in mdf.groups()?.iter().enumerate() {
        for (c_idx, ch) in group.channels.iter().enumerate() {
            if let Some(name) = &ch.name {
                println!("Group {} Channel {}: {}", g_idx + 1, c_idx + 1, name);
            }
        }
    }

    println!("Created {input} and {output}");
    Ok(())
}
