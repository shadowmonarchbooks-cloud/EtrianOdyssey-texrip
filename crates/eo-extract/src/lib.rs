use eo_archives::ExtractionBudget;
use eo_rom::NativeRom;
use eo_untold::{inventory_reader, ParityAsset, ScanIssue, UntoldError};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::{Path, PathBuf}};
use thiserror::Error;

pub const REPORT_NAME: &str = "extraction-report.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportedTexture {
    pub file: String,
    pub source: String,
    pub internal_name: String,
    pub candidate_hash: String,
    pub width: u32,
    pub height: u32,
    pub format: i32,
    pub parser_used: String,
    pub category: String,
    pub material_binding_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractionReport {
    pub schema: String,
    pub profile_id: String,
    pub game_id: String,
    pub title_id: Option<String>,
    pub product_code: Option<String>,
    pub textures_written: usize,
    pub issues: Vec<ScanIssue>,
    pub textures: Vec<ExportedTexture>,
}

#[derive(Debug, Error)]
pub enum ExtractionError {
    #[error("could not read/write extraction files: {0}")]
    Io(#[from] io::Error),
    #[error("could not parse decrypted ROM: {0}")]
    Rom(#[from] eo_rom::RomError),
    #[error("unsupported or invalid Untold ROM: {0}")]
    Untold(#[from] UntoldError),
    #[error("could not write extraction report: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn default_output_path(source: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "eo-rom".to_owned());
    source.with_file_name(format!("{stem}-textures"))
}

pub fn extract_rom_to_directory(
    source: &Path,
    output: &Path,
) -> Result<ExtractionReport, ExtractionError> {
    let bytes = fs::read(source)?;
    let rom = NativeRom::detect(&bytes)?;
    let inventory = inventory_reader(&rom, ExtractionBudget::default())?;
    export_inventory(
        &inventory.assets,
        &inventory.issues,
        output,
        ExtractionIdentity {
            profile_id: inventory.profile_id,
            game_id: format!("{:?}", inventory.game_id),
            title_id: inventory.title_id,
            product_code: inventory.product_code,
        },
    )
}

#[derive(Debug)]
struct ExtractionIdentity {
    profile_id: String,
    game_id: String,
    title_id: Option<String>,
    product_code: Option<String>,
}

fn export_inventory(
    assets: &[ParityAsset],
    issues: &[ScanIssue],
    output: &Path,
    identity: ExtractionIdentity,
) -> Result<ExtractionReport, ExtractionError> {
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
        schema: "eo-texrip-native-extraction-report-v1".to_owned(),
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
    fn default_output_sits_beside_rom() {
        assert_eq!(
            default_output_path(Path::new(r"C:\Games\EOU.3ds")),
            PathBuf::from(r"C:\Games\EOU-textures")
        );
    }

    #[test]
    fn filename_sanitizer_handles_windows_reserved_names() {
        assert_eq!(safe_filename_component("CON"), "_CON");
        assert_eq!(safe_filename_component("LPT9.png"), "_LPT9.png");
        assert_eq!(safe_filename_component("boss:face?01"), "boss_face_01");
    }

    #[test]
    fn png_writer_emits_rgba8_container() {
        let rgba = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let png = encode_png_rgba8(2, 1, &rgba).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..20], &2u32.to_be_bytes());
        assert_eq!(&png[20..24], &1u32.to_be_bytes());
        assert_eq!(png[24], 8);
        assert_eq!(png[25], 6);
    }
}
