use clap::Parser;
use std::path::{Path, PathBuf};
use std::process;

/// JSON with Comments Editor - Edit JSON files with comments
///
/// The tool stores comments in a .jsonc file and maintains clean JSON in the original file.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// The file to edit (can be .json or .jsonc)
    file: String,
}

fn main() {
    let cli = Cli::parse();
    let file_arg = &cli.file;

    // Determine the JSON path
    let json_path = if file_arg.ends_with(".jsonc") {
        // User specified a .jsonc file, get the corresponding .json path
        let stem = Path::new(file_arg)
            .file_stem()
            .unwrap_or_default();
        let parent = Path::new(file_arg)
            .parent()
            .unwrap_or(Path::new("."));
        parent.join(format!("{}.json", stem.to_string_lossy()))
    } else {
        PathBuf::from(file_arg)
    };

    if let Err(e) = jsonc::edit_json(&json_path) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
