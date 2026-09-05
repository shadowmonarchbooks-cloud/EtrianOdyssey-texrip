use eo_recon::recon_reader;
use eo_rom::{NativeRom, RomReader};
use eo_textures::ctpk::{parse_ctpk, CtpkTextureType};
use serde::Serialize;
use std::{collections::BTreeMap, env, error::Error, fs, path::PathBuf, process::ExitCode};

#[derive(Debug, Default, Serialize)]
struct CtpkReconSummary {
    files_seen: u64,
    files_parsed: u64,
    parse_errors: u64,
    textures: u64,
    two_dimensional: u64,
    cube_maps: u64,
    one_dimensional: u64,
    unknown_types: u64,
    formats: BTreeMap<String, u64>,
}

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
    let ctpk = recon_ctpk(&rom)?;

    let mut value = serde_json::to_value(&report)?;
    let object = value
        .as_object_mut()
        .ok_or("recon report did not serialize as a JSON object")?;
    object.insert("ctpk_structural".to_owned(), serde_json::to_value(&ctpk)?);

    let mut json = serde_json::to_vec_pretty(&value)?;
    json.push(b'\n');
    fs::write(&output, json)?;

    println!(
        "{} reconnaissance complete: {} RomFS files, {} HPI members, {} EPL members, {} CTPK textures, report {}",
        report.profile_id,
        report.romfs_files,
        report.archives.hpi_members,
        report.archives.epl_members,
        ctpk.textures,
        output.display()
    );
    Ok(())
}

fn recon_ctpk<R: RomReader>(reader: &R) -> Result<CtpkReconSummary, Box<dyn Error>> {
    let mut summary = CtpkReconSummary::default();
    for entry in reader.entries()? {
        if !entry
            .virtual_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&entry.virtual_path)
            .to_ascii_lowercase()
            .ends_with(".ctpk")
        {
            continue;
        }
        summary.files_seen = summary.files_seen.saturating_add(1);
        let data = match reader.read_entry(&entry.virtual_path) {
            Ok(data) => data,
            Err(_) => {
                summary.parse_errors = summary.parse_errors.saturating_add(1);
                continue;
            }
        };
        let container = match parse_ctpk(&data) {
            Ok(container) => container,
            Err(_) => {
                summary.parse_errors = summary.parse_errors.saturating_add(1);
                continue;
            }
        };
        summary.files_parsed = summary.files_parsed.saturating_add(1);
        for texture in container.textures {
            summary.textures = summary.textures.saturating_add(1);
            *summary
                .formats
                .entry(format!("0x{:02x}", texture.format_raw))
                .or_default() += 1;
            match texture.texture_type {
                CtpkTextureType::TwoDimensional => {
                    summary.two_dimensional = summary.two_dimensional.saturating_add(1)
                }
                CtpkTextureType::CubeMap => {
                    summary.cube_maps = summary.cube_maps.saturating_add(1)
                }
                CtpkTextureType::OneDimensional => {
                    summary.one_dimensional = summary.one_dimensional.saturating_add(1)
                }
                CtpkTextureType::Unknown(_) => {
                    summary.unknown_types = summary.unknown_types.saturating_add(1)
                }
            }
        }
    }
    Ok(summary)
}
