# Picture Reader - Specification

## Project Overview
- **Project name**: picture-reader
- **Type**: Rust CLI application
- **Core functionality**: Read and display information about image files from a local folder
- **Target users**: Developers or users who need to quickly inspect image files in a directory

## Functionality Specification

### Core Features
1. **Scan directory for images**: Recursively or non-recursively scan a specified folder for image files
2. **Supported formats**: JPG/JPEG, PNG, GIF, BMP, WEBP, TIFF
3. **Extract metadata**: For each image, extract:
   - File name
   - File path
   - File size (in bytes, KB, or MB)
   - Image dimensions (width x height)
   - Image format/type
4. **Command-line interface**: Accept folder path as argument
5. **Output options**: Display results in formatted table or JSON

### User Interactions
- User provides folder path as command-line argument
- Optional flags:
  - `-r` / `--recursive`: Scan subdirectories recursively
  - `-j` / `--json`: Output in JSON format
  - `-s` / `--sort`: Sort by file size

### Edge Cases
- Handle empty folders gracefully
- Handle non-existent folder paths
- Handle folders with no image files
- Handle permission errors

## Technical Implementation
- Use `image` crate for reading image metadata
- Use `clap` for CLI argument parsing
- Use `serde_json` for JSON output

## Acceptance Criteria
1. Application compiles without errors
2. Can scan a folder and list all image files
3. Displays correct file size and dimensions for each image
4. Handles edge cases (empty folder, no images, invalid path)
5. Supports both regular and recursive scanning
6. Supports JSON output format
