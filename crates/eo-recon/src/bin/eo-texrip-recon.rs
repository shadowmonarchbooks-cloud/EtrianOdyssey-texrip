use eo_recon::recon_reader;
use eo_rom::NativeRom;
use std::{env, error::Error, fs, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("EO-TexRip recon failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let source = args.next().map(PathBuf::from).ok_or(
        "usage: eo-texrip-recon <decrypted EO4/EO5/EON ROM> [output-report.json]",
    )?;
    let output = args.next().map(PathBuf::from).unwrap_or_else(|| {
        let stem = source
            .file_stem()
            .map(|value| value.to_string_lossy().into_owned())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "eo-rom".to_owned());
        PathBuf::from(format!("{stem}-universal-eo-recon.json"))
    });
    if args.next().is_some() {
        return Err(
            "usage: eo-texrip-recon <decrypted EO4/EO5/EON ROM> [output-report.json]".into(),
        );
    }

    let bytes = fs::read(&source)?;
    let rom = NativeRom::detect(&bytes)?;
    let report = recon_reader(&rom)?;
    let mut json = serde_json::to_vec_pretty(&report)?;
    json.push(b'\n');
    fs::write(&output, json)?;

    println!(
        "{} reconnaissance complete: {} RomFS files, report {}",
        report.profile_id,
        report.romfs_files,
        output.display()
    );
    Ok(())
}
