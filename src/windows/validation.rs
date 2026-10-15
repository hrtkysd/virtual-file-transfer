use crate::{Error, InputError, PlatformError, PlatformErrorKind, Result, VirtualFile};
use std::collections::HashSet;
use windows::Win32::Foundation::MAX_PATH;

pub(crate) fn validate_file_name(name: &str) -> Result<()> {
    // FILEDESCRIPTORW::cFileName includes the terminating NUL.
    if name.encode_utf16().count() >= MAX_PATH as usize {
        return Err(unsupported(name, "path exceeds FILEDESCRIPTORW limit"));
    }

    // Both separators are treated as directory separators.
    for component in name.split(['/', '\\']) {
        validate_component(component).map_err(|reason| unsupported(name, reason))?;
    }

    Ok(())
}

pub(crate) fn validate_files(files: &Vec<VirtualFile>) -> Result<()> {
    let mut paths = HashSet::with_capacity(files.len());

    for file in files {
        let path = normalize_path(file.name());

        if !paths.insert(path) {
            return Err(Error::InvalidInput(InputError::DuplicatePath));
        }
    }

    // A file cannot also be a parent directory.
    for path in &paths {
        for (index, ch) in path.char_indices() {
            if ch == '/' && paths.contains(&path[..index]) {
                return Err(Error::InvalidInput(InputError::ConflictPath));
            }
        }
    }

    Ok(())
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

fn validate_component(component: &str) -> std::result::Result<(), &'static str> {
    if component.is_empty() || matches!(component, "." | "..") {
        return Err("invalid path component");
    }

    if component.ends_with(' ') || component.ends_with('.') {
        return Err("trailing space or trailing period");
    }

    if component.chars().any(is_invalid_character) {
        return Err("invalid character");
    }

    if is_reserved_name(component) {
        return Err("reserved device name");
    }

    Ok(())
}

fn is_invalid_character(ch: char) -> bool {
    matches!(
        ch,
        '\0'..='\u{1f}' | '<' | '>' | ':' | '"' | '|' | '?' | '*'
    )
}

fn is_reserved_name(component: &str) -> bool {
    let upper = component.to_ascii_uppercase();

    if matches!(upper.as_str(), "CONIN$" | "CONOUT$") {
        return true;
    }

    let stem = upper
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ');

    if matches!(stem, "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }

    let suffix = stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"));

    suffix.is_some_and(|digit| {
        matches!(
            digit,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    })
}

fn unsupported(name: &str, reason: &str) -> Error {
    Error::Platform(PlatformError::new(
        PlatformErrorKind::UnsupportedFileName,
        format!("unsupported Windows path {name:?}: {reason}"),
    ))
}
