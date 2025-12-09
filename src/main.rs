use clap::Parser;
use regex::Regex;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{self};
use tempfile::NamedTempFile;

const DEFAULT_TEMPLATE: &str = r#"{
  // Add your configuration here
  "example": "value"
}"#;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// The file to edit
    file: String,
}

fn main() {
    let cli = Cli::parse();
    let file_arg = &cli.file;
    let json_path = PathBuf::from(file_arg); // Strict matching

    if let Err(e) = edit_json(&json_path) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn edit_json(json_path: &Path) -> io::Result<()> {
    // Calculate .jsonc path
    let jsonc_path = get_jsonc_path(json_path);

    // 1. Load Content
    // Rules:
    // - If target (.json) exists, verify if .jsonc exists. Use .jsonc if available, else .json.
    // - If target missing:
    //    - If .jsonc exists -> Error (Ambiguous collision).
    //    - If .jsonc missing -> New Template.
    
    let exists = json_path.exists();
    let is_new_file = !exists;

    let content = if exists {
        if jsonc_path.exists() {
            fs::read_to_string(&jsonc_path)?
        } else {
            fs::read_to_string(json_path)?
        }
    } else {
        if jsonc_path.exists() {
             return Err(io::Error::new(io::ErrorKind::AlreadyExists, 
                format!("Target file {} does not exist, but {} already exists. Aborting.", 
                json_path.display(), jsonc_path.display())
             ));
        }

        DEFAULT_TEMPLATE.to_string()
    };

    // 2. Prepare Temp File
    let mut temp_file = NamedTempFile::with_suffix(".jsonc")?;
    temp_file.write_all(content.as_bytes())?;
    let temp_path_obj = temp_file.into_temp_path();
    let temp_path = temp_path_obj.to_path_buf();

    // Capture initial mtime (to abort creation if not saved)
    let initial_mtime = fs::metadata(&temp_path).and_then(|m| m.modified()).ok();

    // 3. Open Editor (Blocking)
    edit::edit_file(&temp_path)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to open editor: {}", e)))?;

    // 4. Check for Save (Abort if new file wasn't saved)
    let final_mtime = fs::metadata(&temp_path).and_then(|m| m.modified()).ok();
    
    if is_new_file && initial_mtime.is_some() && initial_mtime == final_mtime {
        println!("File was not saved (content unchanged). Aborting creation.");
        return Ok(());
    }

    // 5. Save/Sync
    // Read temp file
    let new_content = fs::read_to_string(&temp_path)?;
    
    // Strip comments to validate and generate cleaner JSON
    let stripped = strip_comments(&new_content);
    
    // Validate JSON
    let parsed: serde_json::Value = match serde_json::from_str(&stripped) {
        Ok(v) => v,
        Err(e) => {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid JSON: {}", e)));
        }
    };
    let pretty_json = serde_json::to_string_pretty(&parsed).unwrap();

    // Write to .jsonc (With comments)
    fs::write(&jsonc_path, &new_content)?;
    
    // Write to .json (Clean)
    fs::write(json_path, pretty_json)?;

    if is_new_file {
        println!("Creating new files: {} and {}", json_path.display(), jsonc_path.display());
    } else {
        println!("Saved {} (clean) and {} (with comments)", json_path.display(), jsonc_path.display());
    }

    Ok(())
}

fn get_jsonc_path(json_path: &Path) -> PathBuf {
    let path_str = json_path.to_string_lossy();
    if path_str.ends_with(".json") {
        PathBuf::from(path_str.replace(".json", ".jsonc"))
    } else {
        PathBuf::from(format!("{}.jsonc", path_str))
    }
}

/// Remove // and /* */ style comments
fn strip_comments(text: &str) -> String {
    let block_re = Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let text = block_re.replace_all(text, "");

    let mut result = Vec::new();
    for line in text.lines() {
        let mut in_string = false;
        let mut escape = false;
        let mut processed_line = String::new();
        let mut chars = line.chars().peekable();

        while let Some(c) = chars.next() {
            if escape {
                processed_line.push(c);
                escape = false;
                continue;
            }
            if c == '\\' {
                processed_line.push(c);
                escape = true;
                continue;
            }
            if c == '"' {
                in_string = !in_string;
                processed_line.push(c);
                continue;
            }
            if !in_string && c == '/' {
                if let Some(&next_char) = chars.peek() {
                    if next_char == '/' {
                        break; // Stop processing this line
                    }
                }
            }
            processed_line.push(c);
        }
        result.push(processed_line);
    }
    result.join("\n")
}
