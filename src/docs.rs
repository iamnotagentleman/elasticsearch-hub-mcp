use std::fs;
use std::path::Path;

/// Read the global docs file.
pub fn get_docs(docs_file: &Path) -> String {
    if !docs_file.exists() {
        return "(No documentation yet. Use write_docs to add setup info.)".to_string();
    }
    match fs::read_to_string(docs_file) {
        Ok(content) => content,
        Err(e) => format!("Error reading docs file: {}", e),
    }
}

/// Overwrite the global docs file.
pub fn write_docs(docs_file: &Path, content: &str) -> String {
    match fs::write(docs_file, content) {
        Ok(_) => "Documentation written.".to_string(),
        Err(e) => format!("Error writing docs file: {}", e),
    }
}

/// Append to the global docs file.
pub fn append_docs(docs_file: &Path, content: &str) -> String {
    let new_content = if docs_file.exists() {
        match fs::read_to_string(docs_file) {
            Ok(existing) => format!("{}\n{}", existing, content),
            Err(e) => return format!("Error reading docs file: {}", e),
        }
    } else {
        content.to_string()
    };

    match fs::write(docs_file, new_content) {
        Ok(_) => "Documentation appended.".to_string(),
        Err(e) => format!("Error writing docs file: {}", e),
    }
}
