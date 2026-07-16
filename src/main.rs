use clap::Parser;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{self};
use tempfile::{NamedTempFile, TempPath};

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
    if let Err(error) = edit::edit_file(&temp_path) {
        return Err(preserve_edits(
            temp_path_obj,
            io::Error::other(format!("Failed to open editor: {}", error)),
        ));
    }

    // 4. Check for Save (Abort if new file wasn't saved)
    let final_mtime = fs::metadata(&temp_path).and_then(|m| m.modified()).ok();

    if is_new_file && initial_mtime.is_some() && initial_mtime == final_mtime {
        println!("File was not saved (content unchanged). Aborting creation.");
        return Ok(());
    }

    // 5. Save/Sync. Keep the working copy if any part of saving fails so the
    // user's editing session can be recovered.
    let save_result = (|| -> io::Result<(bool, bool)> {
        let new_content = fs::read_to_string(&temp_path)?;
        let clean_json = clean_json_content(&new_content)?;

        // Write to .jsonc (With comments)
        let jsonc_changed = write_if_changed(&jsonc_path, &new_content)?;

        // Write to .json (Clean)
        let json_changed = write_if_changed(json_path, &clean_json)?;

        Ok((jsonc_changed, json_changed))
    })();

    let (jsonc_changed, json_changed) = match save_result {
        Ok(changes) => changes,
        Err(error) => return Err(preserve_edits(temp_path_obj, error)),
    };

    if is_new_file {
        println!(
            "Creating new files: {} and {}",
            json_path.display(),
            jsonc_path.display()
        );
    } else if json_changed && jsonc_changed {
        println!(
            "Saved {} (clean) and {} (with comments)",
            json_path.display(),
            jsonc_path.display()
        );
    } else if json_changed {
        println!(
            "Saved {} (clean); {} unchanged",
            json_path.display(),
            jsonc_path.display()
        );
    } else if jsonc_changed {
        println!(
            "Saved {} (with comments); {} unchanged",
            jsonc_path.display(),
            json_path.display()
        );
    } else {
        println!("No changes; files left untouched.");
    }

    Ok(())
}

fn write_if_changed(path: &Path, content: &str) -> io::Result<bool> {
    match fs::read(path) {
        Ok(existing) if existing == content.as_bytes() => Ok(false),
        Ok(_) => {
            fs::write(path, content)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::write(path, content)?;
            Ok(true)
        }
        Err(error) => Err(error),
    }
}

fn preserve_edits(temp_path: TempPath, error: io::Error) -> io::Error {
    let error_kind = error.kind();
    let path = temp_path.to_path_buf();

    match temp_path.keep() {
        Ok(path) => io::Error::new(
            error_kind,
            format!("{}. Edits preserved at {}", error, path.display()),
        ),
        Err(persist_error) => io::Error::new(
            error_kind,
            format!(
                "{}. Failed to preserve edits at {}: {}",
                error,
                path.display(),
                persist_error
            ),
        ),
    }
}

fn clean_json_content(text: &str) -> io::Result<String> {
    let stripped = strip_comments(text)?;

    // Parse only to validate that removing comments produced strict JSON.
    serde_json::from_str::<serde_json::Value>(&stripped)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Invalid JSON: {}", e)))?;

    Ok(stripped)
}

fn get_jsonc_path(json_path: &Path) -> PathBuf {
    if json_path.file_name().is_some_and(|name| name == ".json") {
        json_path.with_file_name(".jsonc")
    } else if json_path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        json_path.with_extension("jsonc")
    } else {
        let mut jsonc_path = json_path.as_os_str().to_os_string();
        jsonc_path.push(".jsonc");
        PathBuf::from(jsonc_path)
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
    let mut line_start = 0;
    let mut remove_commented_line = false;
    let mut block_comment_indent: Option<String> = None;
    let mut pending_block_indent: Option<String> = None;
    let mut block_opening_indent = String::new();
    let mut multiline_inline_block = false;

    while let Some(c) = chars.next() {
        if in_line_comment {
            if c == '\r' || c == '\n' {
                if !remove_commented_line {
                    result.push(c);
                }
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                    if !remove_commented_line {
                        result.push('\n');
                    }
                }
                in_line_comment = false;
                remove_commented_line = false;
                line_start = result.len();
            }
            continue;
        }

        if in_block_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
                pending_block_indent = block_comment_indent.take().or_else(|| {
                    multiline_inline_block.then(|| std::mem::take(&mut block_opening_indent))
                });
                multiline_inline_block = false;
            } else if block_comment_indent.is_none()
                && !multiline_inline_block
                && (c == '\n' || c == '\r')
            {
                let trimmed_len = result.trim_end_matches([' ', '\t']).len();
                result.truncate(trimmed_len);
                result.push(c);
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                    result.push('\n');
                }
                line_start = result.len();
                multiline_inline_block = true;
            }
            continue;
        }

        if pending_block_indent.is_some() {
            if c == ' ' || c == '\t' {
                continue;
            }
            if c == '\r' || c == '\n' {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                pending_block_indent = None;
                line_start = result.len();
                continue;
            }

            let indent = pending_block_indent.take().unwrap();
            result.push_str(&indent);
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
            remove_commented_line = result[line_start..]
                .chars()
                .all(|character| character == ' ' || character == '\t');
            if remove_commented_line {
                result.truncate(line_start);
            }
            in_line_comment = true;
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            block_opening_indent = result[line_start..]
                .chars()
                .take_while(|character| *character == ' ' || *character == '\t')
                .collect();
            let starts_on_own_line = result[line_start..]
                .chars()
                .all(|character| character == ' ' || character == '\t');
            if starts_on_own_line {
                block_comment_indent = Some(result[line_start..].to_owned());
                result.truncate(line_start);
            } else {
                // An inline block comment acts as whitespace. Add a separator
                // only when the existing prefix does not already provide one.
                let has_separator = result
                    .chars()
                    .next_back()
                    .is_some_and(|character| matches!(character, ' ' | '\t' | '\r' | '\n'));
                if !has_separator {
                    result.push(' ');
                }
            }
            multiline_inline_block = false;
            in_block_comment = true;
        } else {
            result.push(c);
            if c == '\n' || c == '\r' {
                line_start = result.len();
            }
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
    use super::{
        clean_json_content, get_jsonc_path, preserve_edits, strip_comments, write_if_changed,
    };
    use std::fs::{self, FileTimes};
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::NamedTempFile;

    #[test]
    fn unchanged_content_is_not_rewritten() {
        let file = NamedTempFile::new().unwrap();
        fs::write(file.path(), "unchanged\n").unwrap();
        let old_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        file.as_file()
            .set_times(FileTimes::new().set_modified(old_time))
            .unwrap();
        let modified_before = fs::metadata(file.path()).unwrap().modified().unwrap();

        assert!(!write_if_changed(file.path(), "unchanged\n").unwrap());
        assert_eq!(
            fs::metadata(file.path()).unwrap().modified().unwrap(),
            modified_before
        );
    }

    #[test]
    fn changed_content_is_written() {
        let file = NamedTempFile::new().unwrap();
        fs::write(file.path(), "before\n").unwrap();

        assert!(write_if_changed(file.path(), "after\n").unwrap());
        assert_eq!(fs::read_to_string(file.path()).unwrap(), "after\n");
    }

    #[test]
    fn failed_save_preserves_the_edited_temp_file() {
        let file = NamedTempFile::with_suffix(".jsonc").unwrap();
        fs::write(file.path(), "{ invalid json").unwrap();
        let path = file.path().to_path_buf();

        let error = preserve_edits(
            file.into_temp_path(),
            io::Error::new(io::ErrorKind::InvalidData, "Invalid JSON"),
        );

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("Edits preserved at"));
        assert!(error.to_string().contains(&path.display().to_string()));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ invalid json");

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonc_path_replaces_only_the_final_json_suffix() {
        assert_eq!(
            get_jsonc_path(Path::new("foo.json/settings.json")),
            PathBuf::from("foo.json/settings.jsonc")
        );
        assert_eq!(
            get_jsonc_path(Path::new("a.json.json")),
            PathBuf::from("a.json.jsonc")
        );
        assert_eq!(
            get_jsonc_path(Path::new("config")),
            PathBuf::from("config.jsonc")
        );
        assert_eq!(get_jsonc_path(Path::new(".json")), PathBuf::from(".jsonc"));
    }

    #[cfg(unix)]
    #[test]
    fn jsonc_path_preserves_non_utf8_file_names() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let path = Path::new(OsStr::from_bytes(b"name-\xff.json"));
        let jsonc_path = get_jsonc_path(path);

        assert_eq!(jsonc_path.as_os_str().as_bytes(), b"name-\xff.jsonc");
    }

    #[test]
    fn clean_json_removes_full_line_comments_without_blank_lines() {
        let jsonc = concat!(
            "{\n",
            "\t\"name\": \"example\",\n",
            "\t\"scripts\": {\n",
            "\t\t\"dev\": \"vite dev\",\n",
            "\t\t// \"build\": \"vite build\",\n",
            "\t\t\"preview\": \"vite preview\",\n",
            "\t\t\"prepare\": \"prepare\",\n",
            "\t\t//\"check\": \"check\",\n",
            "\t\t\"check:watch\": \"check --watch\"\n",
            "\t}\n",
            "}\n",
        );
        let expected = concat!(
            "{\n",
            "\t\"name\": \"example\",\n",
            "\t\"scripts\": {\n",
            "\t\t\"dev\": \"vite dev\",\n",
            "\t\t\"preview\": \"vite preview\",\n",
            "\t\t\"prepare\": \"prepare\",\n",
            "\t\t\"check:watch\": \"check --watch\"\n",
            "\t}\n",
            "}\n",
        );

        assert_eq!(clean_json_content(jsonc).unwrap(), expected);
    }

    #[test]
    fn clean_json_collapses_standalone_multiline_block_comments() {
        let jsonc = concat!(
            "{\n",
            "  \"name\": \"vending-operations-portal\",\n",
            "  /*\"private\": true,\n",
            "  \"version\": \"0.0.1\",\n",
            "  \"type\": \"module\",\n",
            "  */ \"scripts\": {\n",
            "    \"dev\": \"vite dev\"\n",
            "  }\n",
            "}\n",
        );
        let expected = concat!(
            "{\n",
            "  \"name\": \"vending-operations-portal\",\n",
            "  \"scripts\": {\n",
            "    \"dev\": \"vite dev\"\n",
            "  }\n",
            "}\n",
        );

        assert_eq!(clean_json_content(jsonc).unwrap(), expected);
    }

    #[test]
    fn clean_json_collapses_multiline_block_comments_between_tokens() {
        let jsonc = concat!(
            "{ /*\n",
            "  \"settings\": {\n",
            "    \"option1\": true,\n",
            "    \"option2\": false,\n",
            "    \"option3\": null\n",
            "  }*/}\n",
        );
        let expected = "{\n}\n";

        assert_eq!(clean_json_content(jsonc).unwrap(), expected);
    }

    #[test]
    fn inline_block_comments_preserve_surrounding_content() {
        let jsonc = "{\r\n  \"enabled\": /* explanation */ true\r\n}\r\n";
        let expected = "{\r\n  \"enabled\":  true\r\n}\r\n";

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
