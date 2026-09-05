use eo_extract::{default_output_path, extract_rom_to_directory, REPORT_NAME};
use std::{env, error::Error, ffi::OsString, path::{Path, PathBuf}, process};

fn main() {
    if let Err(error) = run() {
        eprintln!("eo-texrip-cli: {error}");
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
    let report = extract_rom_to_directory(&source, &output)?;

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
    "Usage:\n  eo-texrip-cli extract <decrypted-rom> [-o|--output <directory>]\n\nIf --output is omitted, textures are written beside the ROM in <rom-name>-textures."
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
    Ok(output.unwrap_or_else(|| default_output_path(source)))
}
