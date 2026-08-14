//! WASI Preview 2 port of `examples/visualize_layout.rs`.
//!
//! Writes a small MDF file, then asks the imported `mdf` reader for a JSON
//! description of its physical block layout (`file-layout-json`), prints it,
//! and saves it to `example.layout.json` in the preopened directory.

wit_bindgen::generate!({
    world: "mf4-consumer",
    path: "../../wasi/wit",
});

use mf4::core::reader::Mdf;
use mf4::core::types::{DataType, DecodedValue};
use mf4::core::writer::MdfWriter;

fn main() {
    if let Err(err) = run() {
        eprintln!("visualize_layout failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let path = "layout_example.mf4";

    // Create a small file so the example is self-contained.
    let writer = MdfWriter::new();
    writer.init_mdf_file()?;
    let cg = writer.add_channel_group(Some("Group1"))?;
    writer.add_time_channel(&cg, "Time")?;
    writer.add_channel(&cg, "Value", DataType::UnsignedIntegerLe)?;
    writer.start_data_block(&cg)?;
    for i in 0u64..8 {
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

    // Describe the physical block layout.
    let mdf = Mdf::from_file(path)?;
    let json = mdf.file_layout_json()?;
    println!("===== File layout (JSON) =====");
    println!("{json}");

    std::fs::write("example.layout.json", &json).map_err(|e| e.to_string())?;
    println!("\nWrote example.layout.json ({} bytes)", json.len());
    Ok(())
}
