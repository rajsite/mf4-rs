//! WASI Preview 2 port of `examples/read_file.rs`.
//!
//! Walks every channel group and channel of `example.mf4` (written by the
//! `write_file` example) through the imported `mdf` reader, printing metadata
//! and a preview of the decoded samples. Sample decoding is on demand, exactly
//! as in the native example.

wit_bindgen::generate!({
    world: "mf4-consumer",
    path: "../../wasi/wit",
});

use mf4::core::reader::Mdf;
use mf4::core::types::DecodedValue;

fn main() {
    if let Err(err) = run() {
        eprintln!("read_file failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    // Assumes `write_file` has been run to create the file.
    let path = "example.mf4";
    let mdf = Mdf::from_file(path)?;
    println!();

    for group in mdf.groups()? {
        let group_name = group.name.as_deref().unwrap_or("<unnamed>");
        println!("Channel Group Name : {group_name}");
        if let Some(comment) = &group.comment {
            println!("Channel Group Comment : {comment}");
        }
        println!("Record count : {}", group.record_count);
        println!();

        println!("Channels:");
        for channel in &group.channels {
            println!();
            let name = channel.name.as_deref().unwrap_or("<unnamed>");
            println!("    Channel Name {name}");
            if let Some(unit) = &channel.unit {
                println!("    Channel Unit [{unit}]");
            }
            if let Some(comment) = &channel.comment {
                println!("    Comment: {comment}");
            }
            println!(
                "    Data type: {:?}{}",
                channel.data_type,
                if channel.is_master { " (master)" } else { "" }
            );

            // Decode samples on demand via the full signal.
            let signal = mdf.read(name, Some(group_name))?;
            let total = signal.values.len();
            println!("    Samples: {total} records");
            println!("    Values: first 5 = {}", preview(&signal.values, 0, 5));
            println!(
                "    Values: last 5 = {}",
                preview(&signal.values, total.saturating_sub(5), total)
            );
        }
        println!();
    }

    Ok(())
}

/// Render `values[start..end]` as a compact debug string.
fn preview(values: &[Option<DecodedValue>], start: usize, end: usize) -> String {
    let slice = &values[start.min(values.len())..end.min(values.len())];
    let items: Vec<String> = slice.iter().map(render).collect();
    format!("[{}]", items.join(", "))
}

fn render(value: &Option<DecodedValue>) -> String {
    match value {
        None => "None".to_string(),
        Some(DecodedValue::UnsignedInteger(v)) => v.to_string(),
        Some(DecodedValue::SignedInteger(v)) => v.to_string(),
        Some(DecodedValue::Float(v)) => format!("{v:?}"),
        Some(DecodedValue::String(v)) => format!("{v:?}"),
        Some(DecodedValue::ByteArray(v))
        | Some(DecodedValue::MimeSample(v))
        | Some(DecodedValue::MimeStream(v)) => format!("{} bytes", v.len()),
        Some(DecodedValue::Unknown) => "Unknown".to_string(),
    }
}
