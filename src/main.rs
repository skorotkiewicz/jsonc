use clap::Parser;
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
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "Target file {} does not exist, but {} already exists. Aborting.",
                    json_path.display(),
                    jsonc_path.display()
                ),
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
        .map_err(|e| io::Error::other(format!("Failed to open editor: {}", e)))?;

    // 4. Check for Save (Abort if new file wasn't saved)
    let final_mtime = fs::metadata(&temp_path).and_then(|m| m.modified()).ok();

    if is_new_file && initial_mtime.is_some() && initial_mtime == final_mtime {
        println!("File was not saved (content unchanged). Aborting creation.");
        return Ok(());
    }

    // 5. Save/Sync
    // Read temp file
    let new_content = fs::read_to_string(&temp_path)?;

    let clean_json = clean_json_content(&new_content)?;

    // Write to .jsonc (With comments)
    fs::write(&jsonc_path, &new_content)?;

    // Write to .json (Clean)
    fs::write(json_path, clean_json)?;

    if is_new_file {
        println!(
            "Creating new files: {} and {}",
            json_path.display(),
            jsonc_path.display()
        );
    } else {
        println!(
            "Saved {} (clean) and {} (with comments)",
            json_path.display(),
            jsonc_path.display()
        );
    }

    Ok(())
}

fn clean_json_content(text: &str) -> io::Result<String> {
    let stripped = strip_comments(text)?;

    // Parse only to validate that removing comments produced strict JSON.
    serde_json::from_str::<serde_json::Value>(&stripped)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Invalid JSON: {}", e)))?;

    Ok(stripped)
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
fn strip_comments(text: &str) -> io::Result<String> {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while let Some(c) = chars.next() {
        if in_line_comment {
            if c == '\n' || c == '\r' {
                result.push(c);
                in_line_comment = false;
            }
            continue;
        }

        if in_block_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
            } else if c == '\n' || c == '\r' {
                result.push(c);
            }
            continue;
        }

        if in_string {
            result.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }

        if c == '"' {
            result.push(c);
            in_string = true;
        } else if c == '/' && chars.peek() == Some(&'/') {
            chars.next();
            in_line_comment = true;
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            // A block comment acts as whitespace, so keep tokens on either
            // side from being joined into a different value.
            result.push(' ');
            in_block_comment = true;
        } else {
            result.push(c);
        }
    }

    if in_block_comment {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid JSONC: unterminated block comment",
        ));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{clean_json_content, strip_comments};

    #[test]
    fn clean_json_preserves_formatting_except_comments() {
        let jsonc = concat!(
            "{\n",
            "\t\"name\": \"example\",\n",
            "\t\"scripts\": {\n",
            "\t\t\"dev\": \"vite dev\",\n",
            "\t\t// \"test\": \"bun test\",\n",
            "\t\t\"build\": \"vite build\"\n",
            "\t}\n",
            "}\n",
        );
        let expected = concat!(
            "{\n",
            "\t\"name\": \"example\",\n",
            "\t\"scripts\": {\n",
            "\t\t\"dev\": \"vite dev\",\n",
            "\t\t\n",
            "\t\t\"build\": \"vite build\"\n",
            "\t}\n",
            "}\n",
        );

        assert_eq!(clean_json_content(jsonc).unwrap(), expected);
    }

    #[test]
    fn comment_markers_inside_strings_are_preserved() {
        let jsonc = concat!(
            "{\r\n",
            "  \"url\": \"https://example.test/a/*literal*/\" // comment\r\n",
            "}\r\n",
        );
        let expected = concat!(
            "{\r\n",
            "  \"url\": \"https://example.test/a/*literal*/\" \r\n",
            "}\r\n",
        );

        assert_eq!(strip_comments(jsonc).unwrap(), expected);
    }

    #[test]
    fn unterminated_block_comments_are_rejected() {
        let error = clean_json_content("{\"valid\": true} /*").unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "Invalid JSONC: unterminated block comment"
        );
    }
}
