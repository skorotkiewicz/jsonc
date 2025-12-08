# jsonc - JSON with Comments Editor

A robust Rust tool to edit JSON files with comments. It allows you to maintain a commented version of your configuration (`.jsonc`) while automatically generating a strict, clean JSON version (`.json`) for your applications.

## Features

- **Edit with Comments**: Write standard JavaScript-style comments (`//` and `/* */`) in your JSON.
- **Live Sync**: Automatically validates and updates the clean JSON file *every time you save* in your editor.
- **Auto-Strip**: Automatically strips comments to generate valid JSON.
- **Validation**: Ensures your JSON is valid before saving the clean version.
- **Editor Integration**: Opens your default `$EDITOR` (defaults to `nano`) or uses `VISUAL`.

## Usage

```bash
# Edit an existing JSON file
jsonc config.json

# If the file doesn't exist, create it first:
touch config.json
jsonc config.json
```

## Installation

```bash
cargo install --path .
```

## How it works

1. It looks for a `.jsonc` file corresponding to your target `.json`.
   - If `.jsonc` exists, it uses it.
   - If only `.json` exists, it creates `.jsonc` from it.
   - If neither exists, it returns an error (safety feature).
2. It opens the `.jsonc` file in your standard system editor.
3. It **watches the file** for changes. whenever you save in your editor:
   - It validates the syntax.
   - It updates the `.jsonc` file.
   - It strips comments and updates the clean `.json` file immediately.
4. When you close the editor, it performs a final sync.
