//! File-system writes triggered from the frontend. The native save-path
//! picker (`tauri-plugin-dialog`, JS side) only ever returns a path string;
//! actually writing bytes stays Rust-side per the IPC architecture contract.

use crate::error::{AppError, AppResult};

#[tauri::command]
pub fn write_file_bytes(path: String, content_base64: String) -> AppResult<()> {
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, content_base64)
        .map_err(|e| AppError::Other(format!("invalid base64: {e}")))?;
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_decoded_bytes_to_the_given_path() {
        let path = std::env::temp_dir().join("restman_test_write_file_bytes.bin");
        let path_str = path.to_string_lossy().to_string();
        write_file_bytes(path_str.clone(), "aGVsbG8=".into()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn rejects_invalid_base64() {
        let path = std::env::temp_dir().join("restman_test_write_file_bytes_invalid.bin");
        let err = write_file_bytes(path.to_string_lossy().to_string(), "not-base64!!".into());
        assert!(err.is_err());
        assert!(!path.exists());
    }
}
