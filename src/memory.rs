use std::fs;
use std::path::{Path, PathBuf};

const SIZE_LIMIT: usize = 80_000;

fn memory_file(memories_dir: &Path, instance_name: &str) -> PathBuf {
    memories_dir.join(format!("memory_{}.md", instance_name))
}

/// Read all memories for an instance. Returns content or file path notice.
pub fn get_memories(memories_dir: &Path, instance_name: &str) -> String {
    fs::create_dir_all(memories_dir).ok();
    let path = memory_file(memories_dir, instance_name);

    if !path.exists() {
        return "(No memories yet for this instance.)".to_string();
    }

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return format!("Error reading memory file: {}", e),
    };

    if content.len() < SIZE_LIMIT {
        content
    } else {
        format!(
            "Content ({} characters) exceeds maximum allowed {} characters. \
             Memory records at {}, use command line tools to prevent context fill",
            content.len(),
            SIZE_LIMIT,
            path.display()
        )
    }
}

/// Append a memory entry to the instance's memory file.
pub fn write_memory(memories_dir: &Path, instance_name: &str, content: &str) -> String {
    fs::create_dir_all(memories_dir).ok();
    let path = memory_file(memories_dir, instance_name);

    let new_content = if path.exists() {
        match fs::read_to_string(&path) {
            Ok(existing) => format!("{}\n{}", existing, content),
            Err(e) => return format!("Error reading memory file: {}", e),
        }
    } else {
        content.to_string()
    };

    match fs::write(&path, new_content) {
        Ok(_) => format!("Memory saved for instance '{}'.", instance_name),
        Err(e) => format!("Error writing memory file: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_empty_memories() {
        let dir = TempDir::new().unwrap();
        let result = get_memories(dir.path(), "test-instance");
        assert_eq!(result, "(No memories yet for this instance.)");
    }

    #[test]
    fn test_write_and_read_memory() {
        let dir = TempDir::new().unwrap();
        let result = write_memory(
            dir.path(),
            "test-instance",
            "The timestamp field is @timestamp, not timestamp",
        );
        assert!(result.contains("Memory saved"));

        let memories = get_memories(dir.path(), "test-instance");
        assert!(memories.contains("The timestamp field is @timestamp, not timestamp"));
    }

    #[test]
    fn test_multiple_memories() {
        let dir = TempDir::new().unwrap();
        for i in 0..3 {
            write_memory(dir.path(), "test-instance", &format!("Memory {}", i));
        }

        let memories = get_memories(dir.path(), "test-instance");
        for i in 0..3 {
            assert!(memories.contains(&format!("Memory {}", i)));
        }
    }

    #[test]
    fn test_large_memories_return_file_path() {
        let dir = TempDir::new().unwrap();
        for i in 0..500 {
            write_memory(
                dir.path(),
                "test-instance",
                &format!("{} item {}", "x".repeat(200), i),
            );
        }

        let result = get_memories(dir.path(), "test-instance");
        assert!(result.contains("exceeds maximum allowed"));
        assert!(result.contains("memory_test-instance.md"));
    }

    #[test]
    fn test_separate_instance_memories() {
        let dir = TempDir::new().unwrap();
        write_memory(dir.path(), "instance-a", "A info");
        write_memory(dir.path(), "instance-b", "B info");

        let a_memories = get_memories(dir.path(), "instance-a");
        let b_memories = get_memories(dir.path(), "instance-b");

        assert!(a_memories.contains("A info"));
        assert!(b_memories.contains("B info"));
        assert!(!a_memories.contains("B info"));
        assert!(!b_memories.contains("A info"));
    }
}
