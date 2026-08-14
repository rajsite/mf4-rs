//! Parity tests for the in-memory (`*_bytes`) cut/merge entry points.
//!
//! These verify that `cut_mdf_by_time_bytes`, `cut_mdf_by_utc_ns_bytes`, and
//! `merge_files_bytes` produce output byte-identical to their path-based
//! counterparts, so the wasm/WASI component can reuse the same cores with
//! confidence.

use mf4_rs::api::mdf::MDF;
use mf4_rs::blocks::common::DataType;
use mf4_rs::cut::{
    cut_mdf_by_time, cut_mdf_by_time_bytes, cut_mdf_by_utc_ns, cut_mdf_by_utc_ns_bytes,
};
use mf4_rs::error::MdfError;
use mf4_rs::merge::{merge_files, merge_files_bytes};
use mf4_rs::parsing::decoder::DecodedValue;
use mf4_rs::writer::MdfWriter;

/// Write a simple `Time` (f64) + `Val` (u32) file with `n` records at
/// `t = i * 0.1s`, returning its path.
fn write_time_val_file(path: &std::path::Path, n: u64) -> Result<(), MdfError> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let mut writer = MdfWriter::new(path.to_str().unwrap())?;
    writer.init_mdf_file()?;
    let cg_id = writer.add_channel_group(None, |_| {})?;
    let time_id = writer.add_channel(&cg_id, None, |ch| {
        ch.data_type = DataType::FloatLE;
        ch.bit_count = 64;
        ch.name = Some("Time".into());
    })?;
    writer.set_time_channel(&time_id)?;
    writer.add_channel(&cg_id, Some(&time_id), |ch| {
        ch.data_type = DataType::UnsignedIntegerLE;
        ch.bit_count = 32;
        ch.name = Some("Val".into());
    })?;
    writer.start_data_block_for_cg(&cg_id, 0)?;
    for i in 0..n {
        writer.write_record(
            &cg_id,
            &[
                DecodedValue::Float(i as f64 * 0.1),
                DecodedValue::UnsignedInteger(i),
            ],
        )?;
    }
    writer.finish_data_block(&cg_id)?;
    writer.finalize()?;
    Ok(())
}

#[test]
fn cut_by_time_bytes_matches_path() -> Result<(), MdfError> {
    let input = std::env::temp_dir().join("bytes_cut_time_input.mf4");
    let out_path = std::env::temp_dir().join("bytes_cut_time_out.mf4");
    write_time_val_file(&input, 10)?;

    let start = 0.2_f64;
    let end = 0.5_f64;

    cut_mdf_by_time(input.to_str().unwrap(), out_path.to_str().unwrap(), start, end)?;
    let path_bytes = std::fs::read(&out_path)?;

    let input_bytes = std::fs::read(&input)?;
    let mem_bytes = cut_mdf_by_time_bytes(&input_bytes, start, end)?;

    assert_eq!(
        path_bytes, mem_bytes,
        "bytes-based cut must be byte-identical to path-based cut"
    );

    // Sanity: the in-memory output is a readable MDF with the expected window.
    let mdf = MDF::from_bytes(mem_bytes)?;
    let groups = mdf.channel_groups();
    assert_eq!(groups.len(), 1);
    let channels = groups[0].channels();
    let vals = channels
        .iter()
        .find(|c| c.name().ok().flatten().as_deref() == Some("Val"))
        .unwrap()
        .values()?;
    // Records at t = 0.2, 0.3, 0.4, 0.5 => values 2,3,4,5.
    let nums: Vec<u64> = vals
        .into_iter()
        .filter_map(|v| match v {
            Some(DecodedValue::UnsignedInteger(u)) => Some(u),
            _ => None,
        })
        .collect();
    assert_eq!(nums, vec![2, 3, 4, 5]);

    for p in [&input, &out_path] {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

#[test]
fn cut_by_utc_ns_bytes_matches_path() -> Result<(), MdfError> {
    let input = std::env::temp_dir().join("bytes_cut_utc_input.mf4");
    let out_path = std::env::temp_dir().join("bytes_cut_utc_out.mf4");
    write_time_val_file(&input, 10)?;

    let anchor_ns = MDF::from_file(input.to_str().unwrap())?
        .start_time_ns()
        .expect("file should have non-zero abs_time");
    let start_ns = anchor_ns as i64 + (0.2_f64 * 1.0e9).round() as i64;
    let end_ns = anchor_ns as i64 + (0.5_f64 * 1.0e9).round() as i64;

    cut_mdf_by_utc_ns(
        input.to_str().unwrap(),
        out_path.to_str().unwrap(),
        start_ns,
        end_ns,
    )?;
    let path_bytes = std::fs::read(&out_path)?;

    let input_bytes = std::fs::read(&input)?;
    let mem_bytes = cut_mdf_by_utc_ns_bytes(&input_bytes, start_ns, end_ns)?;

    assert_eq!(
        path_bytes, mem_bytes,
        "bytes-based UTC cut must be byte-identical to path-based UTC cut"
    );

    for p in [&input, &out_path] {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

#[test]
fn cut_by_utc_ns_bytes_errors_without_abs_time() -> Result<(), MdfError> {
    // A file whose HD.abs_time is 0 cannot be cut by UTC timestamps.
    let input = std::env::temp_dir().join("bytes_cut_utc_noabs.mf4");
    if input.exists() {
        std::fs::remove_file(&input)?;
    }
    let mut writer = MdfWriter::new(input.to_str().unwrap())?;
    writer.init_mdf_file()?;
    // Force abs_time to 0.
    writer.set_start_time(0, 0, 0, 0, 0)?;
    let cg_id = writer.add_channel_group(None, |_| {})?;
    let time_id = writer.add_channel(&cg_id, None, |ch| {
        ch.data_type = DataType::FloatLE;
        ch.bit_count = 64;
        ch.name = Some("Time".into());
    })?;
    writer.set_time_channel(&time_id)?;
    writer.start_data_block_for_cg(&cg_id, 0)?;
    writer.write_record(&cg_id, &[DecodedValue::Float(0.0)])?;
    writer.finish_data_block(&cg_id)?;
    writer.finalize()?;

    let input_bytes = std::fs::read(&input)?;
    let res = cut_mdf_by_utc_ns_bytes(&input_bytes, 0, 1_000_000_000);
    assert!(
        res.is_err(),
        "UTC cut on a file without abs_time should error"
    );

    let _ = std::fs::remove_file(&input);
    Ok(())
}

#[test]
fn merge_files_bytes_matches_path() -> Result<(), MdfError> {
    let f1 = std::env::temp_dir().join("bytes_merge_1.mf4");
    let f2 = std::env::temp_dir().join("bytes_merge_2.mf4");
    let out_path = std::env::temp_dir().join("bytes_merge_out.mf4");
    for p in [&f1, &f2, &out_path] {
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }

    // Two files with identical layouts (single u32 channel).
    let mut w1 = MdfWriter::new(f1.to_str().unwrap())?;
    w1.init_mdf_file()?;
    let cg1 = w1.add_channel_group(None, |_| {})?;
    w1.add_channel(&cg1, None, |ch| {
        ch.data_type = DataType::UnsignedIntegerLE;
        ch.bit_count = 32;
        ch.name = Some("Val".into());
    })?;
    w1.start_data_block_for_cg(&cg1, 0)?;
    for i in 0..3u64 {
        w1.write_record(&cg1, &[DecodedValue::UnsignedInteger(i)])?;
    }
    w1.finish_data_block(&cg1)?;
    w1.finalize()?;

    let mut w2 = MdfWriter::new(f2.to_str().unwrap())?;
    w2.init_mdf_file()?;
    let cg2 = w2.add_channel_group(None, |_| {})?;
    w2.add_channel(&cg2, None, |ch| {
        ch.data_type = DataType::UnsignedIntegerLE;
        ch.bit_count = 32;
        ch.name = Some("Val".into());
    })?;
    w2.start_data_block_for_cg(&cg2, 0)?;
    for i in 10..13u64 {
        w2.write_record(&cg2, &[DecodedValue::UnsignedInteger(i)])?;
    }
    w2.finish_data_block(&cg2)?;
    w2.finalize()?;

    merge_files(
        out_path.to_str().unwrap(),
        f1.to_str().unwrap(),
        f2.to_str().unwrap(),
    )?;
    let path_bytes = std::fs::read(&out_path)?;

    let b1 = std::fs::read(&f1)?;
    let b2 = std::fs::read(&f2)?;
    let mem_bytes = merge_files_bytes(&b1, &b2)?;

    assert_eq!(
        path_bytes, mem_bytes,
        "bytes-based merge must be byte-identical to path-based merge"
    );

    // Sanity: merged output concatenates both groups' records.
    let mdf = MDF::from_bytes(mem_bytes)?;
    let groups = mdf.channel_groups();
    assert_eq!(groups.len(), 1);
    let vals = groups[0].channels()[0].values()?;
    let nums: Vec<u64> = vals
        .into_iter()
        .filter_map(|v| match v {
            Some(DecodedValue::UnsignedInteger(u)) => Some(u),
            _ => None,
        })
        .collect();
    assert_eq!(nums, vec![0, 1, 2, 10, 11, 12]);

    for p in [&f1, &f2, &out_path] {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}
