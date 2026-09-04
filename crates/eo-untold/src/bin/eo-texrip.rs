use eo_archives::ExtractionBudget;
use eo_rom::NativeRom;
use eo_untold::{inventory_reader, ParityAsset, ScanIssue};
use serde::Serialize;
use std::{
    env,
    error::Error,
    ffi::OsString,
    fs,
    io,
    path::{Path, PathBuf},
    process,
};

const REPORT_NAME: &str = "extraction-report.json";

fn main() {
    if let Err(error) = run() {
        eprintln!("eo-texrip: {error}");
        process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let Some(command) = args.next() else {
        return Err(usage().into());
    };
    if matches!(command.to_str(), Some("-h") | Some("--help")) {
        println!("{}", usage());
        return Ok(());
    }
    if command.to_string_lossy() != "extract" {
        return Err(format!("unknown command {:?}\n\n{}", command, usage()).into());
    }

    let Some(source_arg) = args.next() else {
        return Err(format!("missing decrypted ROM path\n\n{}", usage()).into());
    };
    let source = PathBuf::from(source_arg);
    let output = parse_output_arg(args.collect(), &source)?;

    let bytes = fs::read(&source)?;
    let rom = NativeRom::detect(&bytes)?;
    let inventory = inventory_reader(&rom, ExtractionBudget::default())?;
    let report = export_inventory(
        &inventory.assets,
        &inventory.issues,
        &output,
        ExtractionIdentity {
            profile_id: inventory.profile_id.clone(),
            game_id: format!("{:?}", inventory.game_id),
            title_id: inventory.title_id.clone(),
            product_code: inventory.product_code.clone(),
        },
    )?;

    println!("EO-TexRip native extraction complete");
    println!("  Game profile: {}", report.profile_id);
    println!("  Textures: {}", report.textures_written);
    println!("  Warnings: {}", report.issues.len());
    println!("  Output: {}", output.display());
    println!("  Report: {}", output.join(REPORT_NAME).display());

    if report.textures_written == 0 {
        return Err("no supported textures were decoded from this ROM".into());
    }
    Ok(())
}

fn usage() -> &'static str {
    "Usage:\n  eo-texrip extract <decrypted-rom> [-o|--output <directory>]\n\nIf --output is omitted, textures are written beside the ROM in <rom-name>-textures."
}

fn parse_output_arg(args: Vec<OsString>, source: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let mut output = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-o") | Some("--output") => {
                if output.is_some() {
                    return Err("output directory was specified more than once".into());
                }
                let Some(value) = args.next() else {
                    return Err("--output requires a directory path".into());
                };
                output = Some(PathBuf::from(value));
            }
            Some("-h") | Some("--help") => return Err(usage().into()),
            _ => return Err(format!("unknown argument {:?}\n\n{}", arg, usage()).into()),
        }
    }
    Ok(output.unwrap_or_else(|| default_output(source)))
}

fn default_output(source: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "eo-rom".to_owned());
    source.with_file_name(format!("{stem}-textures"))
}

#[derive(Debug)]
struct ExtractionIdentity {
    profile_id: String,
    game_id: String,
    title_id: Option<String>,
    product_code: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ExportedTexture {
    file: String,
    source: String,
    internal_name: String,
    candidate_hash: String,
    width: u32,
    height: u32,
    format: i32,
    parser_used: String,
    category: String,
    material_binding_count: u64,
}

#[derive(Clone, Debug, Serialize)]
struct ExtractionReport {
    schema: &'static str,
    profile_id: String,
    game_id: String,
    title_id: Option<String>,
    product_code: Option<String>,
    textures_written: usize,
    issues: Vec<ScanIssue>,
    textures: Vec<ExportedTexture>,
}

fn export_inventory(
    assets: &[ParityAsset],
    issues: &[ScanIssue],
    output: &Path,
    identity: ExtractionIdentity,
) -> Result<ExtractionReport, Box<dyn Error>> {
    fs::create_dir_all(output)?;
    let mut textures = Vec::with_capacity(assets.len());

    for asset in assets {
        let category = safe_filename_component(&asset.category);
        let directory = output.join(&category);
        fs::create_dir_all(&directory)?;
        let base = preferred_name(asset);
        let filename = format!(
            "{}__{}x{}_f{}_{}.png",
            safe_filename_component(&base),
            asset.width,
            asset.height,
            asset.format,
            asset.candidate_hash
        );
        let relative = format!("{category}/{filename}");
        let png = encode_png_rgba8(asset.width, asset.height, &asset.rgba8)?;
        fs::write(directory.join(&filename), png)?;
        textures.push(ExportedTexture {
            file: relative,
            source: asset.source.clone(),
            internal_name: asset.internal_name.clone(),
            candidate_hash: asset.candidate_hash.clone(),
            width: asset.width,
            height: asset.height,
            format: asset.format,
            parser_used: asset.parser_used.clone(),
            category: asset.category.clone(),
            material_binding_count: asset.material_binding_count,
        });
    }

    let report = ExtractionReport {
        schema: "eo-texrip-native-extraction-report-v1",
        profile_id: identity.profile_id,
        game_id: identity.game_id,
        title_id: identity.title_id,
        product_code: identity.product_code,
        textures_written: textures.len(),
        issues: issues.to_vec(),
        textures,
    };
    let mut json = serde_json::to_vec_pretty(&report)?;
    json.push(b'\n');
    fs::write(output.join(REPORT_NAME), json)?;
    Ok(report)
}

fn preferred_name(asset: &ParityAsset) -> String {
    let internal = asset.internal_name.trim();
    if !internal.is_empty() {
        return internal.to_owned();
    }
    let normalized = asset.source.replace('\\', "/");
    Path::new(&normalized)
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "texture".to_owned())
}

fn safe_filename_component(value: &str) -> String {
    let mut output = String::new();
    let mut previous_separator = false;
    for ch in value.chars().take(96) {
        let mapped = if ch.is_control()
            || ch.is_whitespace()
            || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        {
            '_'
        } else {
            ch
        };
        if mapped == '_' {
            if previous_separator {
                continue;
            }
            previous_separator = true;
        } else {
            previous_separator = false;
        }
        output.push(mapped);
    }
    let trimmed = output.trim_matches(['.', '_', ' ']);
    let mut result = if trimmed.is_empty() {
        "texture".to_owned()
    } else {
        trimmed.to_owned()
    };
    if is_windows_reserved_name(&result) {
        result.insert(0, '_');
    }
    result
}

fn is_windows_reserved_name(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or(value)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    if stem.len() == 4 {
        let (prefix, number) = stem.split_at(3);
        return matches!(prefix, "COM" | "LPT")
            && matches!(
                number,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"
            );
    }
    false
}

fn encode_png_rgba8(width: u32, height: u32, rgba8: &[u8]) -> io::Result<Vec<u8>> {
    if width == 0 || height == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "PNG dimensions must be non-zero",
        ));
    }
    let row_bytes = usize::try_from(u64::from(width) * 4).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "PNG row size exceeds address space",
        )
    })?;
    let expected = row_bytes.checked_mul(height as usize).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "PNG pixel size overflow")
    })?;
    if rgba8.len() != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "RGBA length mismatch: expected {expected} bytes for {width}x{height}, got {}",
                rgba8.len()
            ),
        ));
    }

    let scanline_bytes = row_bytes.checked_add(1).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "PNG scanline size overflow")
    })?;
    let raw_capacity = scanline_bytes.checked_mul(height as usize).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "PNG image size overflow")
    })?;
    let mut raw = Vec::with_capacity(raw_capacity);
    for row in rgba8.chunks_exact(row_bytes) {
        raw.push(0);
        raw.extend_from_slice(row);
    }

    let compressed = zlib_store(&raw);
    let mut png = Vec::with_capacity(8 + 25 + compressed.len() + 24);
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = [0u8; 13];
    ihdr[..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6;
    append_chunk(&mut png, b"IHDR", &ihdr)?;
    append_chunk(&mut png, b"IDAT", &compressed)?;
    append_chunk(&mut png, b"IEND", &[])?;
    Ok(png)
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(data.len() + (data.len() / 65_535 + 1) * 5 + 6);
    output.extend_from_slice(&[0x78, 0x01]);
    if data.is_empty() {
        output.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        let chunk_count = data.len().div_ceil(65_535);
        for (index, chunk) in data.chunks(65_535).enumerate() {
            output.push(if index + 1 == chunk_count { 0x01 } else { 0x00 });
            let len = chunk.len() as u16;
            output.extend_from_slice(&len.to_le_bytes());
            output.extend_from_slice(&(!len).to_le_bytes());
            output.extend_from_slice(chunk);
        }
    }
    output.extend_from_slice(&adler32(data).to_be_bytes());
    output
}

fn append_chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) -> io::Result<()> {
    let length = u32::try_from(data.len()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "PNG chunk exceeds u32 length")
    })?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(kind);
    output.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(kind.len() + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    output.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    Ok(())
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + u32::from(byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_writer_emits_rgba8_scanlines_in_a_valid_container() {
        let rgba = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let png = encode_png_rgba8(2, 1, &rgba).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");

        let ihdr_len = u32::from_be_bytes(png[8..12].try_into().unwrap()) as usize;
        assert_eq!(ihdr_len, 13);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..20], &2u32.to_be_bytes());
        assert_eq!(&png[20..24], &1u32.to_be_bytes());
        assert_eq!(png[24], 8);
        assert_eq!(png[25], 6);
        let ihdr_crc = u32::from_be_bytes(png[29..33].try_into().unwrap());
        assert_eq!(ihdr_crc, crc32(&png[12..29]));

        let idat_offset = 33;
        let idat_len =
            u32::from_be_bytes(png[idat_offset..idat_offset + 4].try_into().unwrap()) as usize;
        assert_eq!(&png[idat_offset + 4..idat_offset + 8], b"IDAT");
        let idat = &png[idat_offset + 8..idat_offset + 8 + idat_len];
        assert_eq!(&idat[..2], &[0x78, 0x01]);
        let raw = inflate_stored_zlib(idat);
        let mut expected = vec![0];
        expected.extend_from_slice(&rgba);
        assert_eq!(raw, expected);
    }

    #[test]
    fn filename_sanitizer_handles_windows_reserved_names() {
        assert_eq!(safe_filename_component("CON"), "_CON");
        let sanitized = safe_filename_component("boss/face:*?");
        assert!(!sanitized.contains(['/', ':', '*', '?']));
        assert!(!sanitized.is_empty());
    }

    fn inflate_stored_zlib(data: &[u8]) -> Vec<u8> {
        assert!(data.len() >= 6);
        assert_eq!(&data[..2], &[0x78, 0x01]);
        let payload_end = data.len() - 4;
        let mut offset = 2usize;
        let mut output = Vec::new();
        loop {
            let header = data[offset];
            offset += 1;
            assert_eq!((header >> 1) & 0x03, 0);
            let final_block = header & 1 != 0;
            let len = u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap());
            let nlen = u16::from_le_bytes(data[offset + 2..offset + 4].try_into().unwrap());
            offset += 4;
            assert_eq!(nlen, !len);
            let end = offset + usize::from(len);
            assert!(end <= payload_end);
            output.extend_from_slice(&data[offset..end]);
            offset = end;
            if final_block {
                break;
            }
        }
        assert_eq!(offset, payload_end);
        let expected_adler = u32::from_be_bytes(data[payload_end..].try_into().unwrap());
        assert_eq!(expected_adler, adler32(&output));
        output
    }
}
