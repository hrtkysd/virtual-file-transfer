use crate::{Error, FileNameErrorReason, InputError, Result, VirtualFile};
use std::collections::HashSet;
use windows::Win32::Foundation::MAX_PATH;

pub(crate) fn validate_file_name(name: &str) -> Result<()> {
    // FILEDESCRIPTORW::cFileName includes the terminating NUL.
    let actual = name.encode_utf16().count();
    let max = MAX_PATH as usize - 1;

    if actual > max {
        return Err(Error::InvalidInput(InputError::InvalidFileName {
            name: name.to_owned(),
            reason: FileNameErrorReason::TooLong { actual, max },
        }));
    }
    // Both separators are treated as directory separators.
    for component in name.split(['/', '\\']) {
        validate_component(component).map_err(|reason| {
            Error::InvalidInput(InputError::InvalidFileName {
                name: name.to_owned(),
                reason,
            })
        })?;
    }

    Ok(())
}

pub(crate) fn validate_files(files: &[VirtualFile]) -> Result<()> {
    if files.is_empty() {
        return Err(Error::InvalidInput(InputError::EmptyTransfer));
    }

    let mut paths = HashSet::with_capacity(files.len());

    for file in files {
        let path = normalize_path(file.name());

        if !paths.insert(path) {
            return Err(Error::InvalidInput(InputError::DuplicatePath));
        }
    }

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

fn validate_component(component: &str) -> std::result::Result<(), FileNameErrorReason> {
    if component.is_empty() || matches!(component, "." | "..") {
        return Err(FileNameErrorReason::InvalidComponent(component.to_owned()));
    }

    if component.ends_with(' ') || component.ends_with('.') {
        return Err(FileNameErrorReason::TrailingSpaceOrPeriod(
            component.to_owned(),
        ));
    }

    if let Some(ch) = component.chars().find(|&ch| is_invalid_character(ch)) {
        return Err(FileNameErrorReason::InvalidCharacter(ch));
    }

    if is_reserved_name(component) {
        return Err(FileNameErrorReason::ReservedName(component.to_owned()));
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
