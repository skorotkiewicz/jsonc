# jsonc - JSON with Comments Editor

A robust Rust tool to edit JSON files with comments. It allows you to maintain a commented version of your configuration (`.jsonc`) while automatically generating a strict, clean JSON version (`.json`) for your applications.

## Features

- **Edit with Comments**: Write standard JavaScript-style comments (`//` and `/* */`) in your JSON.
- **Auto-Strip**: Automatically strips comments to generate valid JSON.
- **Validation**: Ensures your JSON is valid before saving the clean version.
- **Editor Integration**: Opens your default `$EDITOR` (defaults to `nano`) or uses `VISUAL`.

## Usage

```bash
# If the file doesn't exist, it creates one (with a default template)
jsonc new_config.json


# View the clean JSON output
cat config.json
```

## Installation

```bash
cargo install --path .
```

## How it works

1. When you run `jsonc config.json`, it opens the `.jsonc` version of your file (where your comments live).
2. If the file is new, it creates it for you (after you save). If you have an existing `.json` file, it automatically creates a commented version so you can start editing immediately.
3. **Collision Safety**: If the target `.json` file does NOT exist, but a corresponding `.jsonc` file DOES exist (e.g., from a naming collision), it will **return an error** to prevent accidental overwriting.
4. It opens the `.jsonc` file in your standard system editor.
5. **Save and Close**: When you are finished, just save and exit the editor. `jsonc` will automatically:
   - Validate your JSON.
   - Save your comments to the `.jsonc` file.
   - Strip comments and save the clean JSON to your target file.

> **Tip**: If you exit a new file without saving, `jsonc` will abort and nothing will be created.
