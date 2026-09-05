use eo_archives::ExtractionBudget;
use eo_rom::NativeRom;
use eo_untold::{compare_fingerprints, inventory_reader, StructuralFingerprint};
use std::{env, fs, process};

fn main() {
    if let Err(error) = run() {
        eprintln!("untold-fingerprint: {error}");
        process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let Some(source_path) = args.next() else {
        return Err(
            "usage: untold-fingerprint <decrypted-ncsd|cia|ncch> [frozen-schema1.json]".into(),
        );
    };
    let expected_path = args.next();
    if args.next().is_some() {
        return Err(
            "usage: untold-fingerprint <decrypted-ncsd|cia|ncch> [frozen-schema1.json]".into(),
        );
    }

    let source = fs::read(&source_path)?;
    let rom = NativeRom::detect(&source)?;
    let inventory = inventory_reader(&rom, ExtractionBudget::default())?;
    let actual = inventory.structural_fingerprint();

    println!("{}", serde_json::to_string_pretty(&actual)?);

    if let Some(expected_path) = expected_path {
        let expected: StructuralFingerprint =
            serde_json::from_slice(&fs::read(&expected_path)?)?;
        let comparison = compare_fingerprints(&expected, &actual);
        eprintln!("{}", serde_json::to_string_pretty(&comparison)?);
        if !comparison.matches {
            process::exit(1);
        }
    }

    Ok(())
}
