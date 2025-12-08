use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

/// Remove // and /* */ style comments from JSON text
pub fn strip_comments(text: &str) -> String {
    // Remove /* */ block comments first
    // (?s) enables "dot matches newline" mode for multi-line block comments
    let block_re = Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let text = block_re.replace_all(text, "");

    // Remove // line comments (but not inside strings)
    let mut result = Vec::new();

    for line in text.lines() {
        let mut in_string = false;
        let mut escape = false;
        let mut cut_index: Option<usize> = None;
        let chars: Vec<char> = line.chars().collect();

        for i in 0..chars.len() {
            let char = chars[i];

            if escape {
                escape = false;
                continue;
            }

            if char == '\\' {
                escape = true;
                continue;
            }

            if char == '"' {
                in_string = !in_string;
            }

            if char == '/' && i + 1 < chars.len() && chars[i + 1] == '/' && !in_string {
                cut_index = Some(i);
                break;
            }
        }

        let processed_line = match cut_index {
            Some(idx) => &line[..line.char_indices().nth(idx).map(|(i, _)| i).unwrap_or(line.len())],
            None => &line[..],
        };
        result.push(processed_line);
    }

    result.join("\n")
}

/// Get the path to the .jsonc file from the .json path
fn get_jsonc_path(json_path: &Path) -> PathBuf {
    let stem = json_path.file_stem().unwrap_or_default();
    let parent = json_path.parent().unwrap_or(Path::new("."));
    parent.join(format!("{}.jsonc", stem.to_string_lossy()))
}

/// Sync content from temp file to destination files
fn sync_content(temp_path: &Path, jsonc_path: &Path, json_path: &Path) -> io::Result<()> {
    // Read edited content
    // We try multiple times in case the editor is currently locking/writing the file
    let new_content;
    let start = Instant::now();
    loop {
        match fs::read_to_string(temp_path) {
            Ok(c) => {
                new_content = c;
                break;
            }
            Err(e) => {
                if start.elapsed() > Duration::from_millis(500) {
                    return Err(e);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    // Validate JSON by stripping comments and parsing
    let stripped = strip_comments(&new_content);
    let data: serde_json::Value = serde_json::from_str(&stripped).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid JSON: {}", e),
        )
    })?;

    // Save .jsonc with comments
    fs::write(jsonc_path, &new_content)?;

    // Save .json without comments (pretty-printed)
    let pretty_json = serde_json::to_string_pretty(&data)?;
    fs::write(json_path, pretty_json)?;

    println!(
        "Synced {} and {}",
        json_path.display(),
        jsonc_path.display()
    );

    Ok(())
}

/// Edit JSON with comments
pub fn edit_json(json_path: &Path) -> io::Result<()> {
    let jsonc_path = get_jsonc_path(json_path);

    // Determine what content to edit
    if !json_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "File not found: {}. Use 'touch {}' to create it first if needed.",
                json_path.display(),
                json_path.display()
            ),
        ));
    }

    let content = if jsonc_path.exists() {
        // Edit existing .jsonc file
        fs::read_to_string(&jsonc_path)?
    } else {
        // Create .jsonc from existing .json
        let content = fs::read_to_string(json_path)?;
        println!("Creating {} from {}", jsonc_path.display(), json_path.display());
        content
    };

    // Get the editor from EDITOR env var, default to nano
    let editor = env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());

    // Create a temporary file with .jsonc extension
    let mut temp_file = NamedTempFile::with_suffix(".jsonc")?;
    temp_file.write_all(content.as_bytes())?;
    
    // Close the file handle (by converting to TempPath) but keep the file on disk
    let temp_path_obj = temp_file.into_temp_path();
    let temp_path = temp_path_obj.to_path_buf();

    // Set up file watcher
    let json_path_clone = json_path.to_path_buf();
    let jsonc_path_clone = jsonc_path.clone();
    let temp_path_clone = temp_path.clone();
    
    // We use a debounce to avoid too many quick writes
    let last_update = Arc::new(Mutex::new(Instant::now()));

    let mut watcher = RecommendedWatcher::new(move |res: Result<Event, notify::Error>| {
        match res {
            Ok(event) => {
                // Check if it's a modify event (or close_write, or sometimes Rename/Create depending on editor)
                let meaningful_change = matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_) | EventKind::Access(notify::event::AccessKind::Close(notify::event::AccessMode::Write)));
                
                if meaningful_change {
                    let mut last = last_update.lock().unwrap();
                    if last.elapsed() < Duration::from_millis(100) {
                        return; // Debounce
                    }
                    *last = Instant::now();
                    
                    // Attempt to sync
                    if let Err(e) = sync_content(&temp_path_clone, &jsonc_path_clone, &json_path_clone) {
                        // Print error but don't crash watcher
                         eprintln!("Error syncing file: {}", e);
                    }
                }
            }
            Err(e) => eprintln!("Watch error: {:?}", e),
        }
    }, Config::default()).ok(); // Ignore watcher init errors, just fallback to save-on-exit

    if let Some(ref mut w) = watcher {
         // Watch the parent directory because checking the file itself usually is flaky with editors that do swap implementation (renaming)
         // But watching parent means we get all events.
         // Let's watch the file directly first. notify usually handles it.
         if let Err(e) = w.watch(&temp_path, RecursiveMode::NonRecursive) {
             eprintln!("Warning: Could not start file watcher: {}", e);
         }
    }

    // Open in editor
    let status = Command::new(&editor)
        .arg(&temp_path)
        .status()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to open editor '{}': {}", editor, e)))?;

    // Drop watcher to stop it
    drop(watcher);

    if !status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, "Editor exited with non-zero status"));
    }

    // Final sync
    sync_content(&temp_path, &jsonc_path, json_path)?;

    Ok(())
}


