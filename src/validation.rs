use crate::{Error, InputError, Result};

pub(crate) fn validate_file_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::InvalidInput(InputError::EmptyName));
    }

    if name.contains('\0') || matches!(name, "." | "..") {
        return Err(Error::InvalidInput(InputError::InvalidFileName));
    }

    #[cfg(windows)]
    crate::windows::validation::validate_file_name(name)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_names() {
        for name in ["file.txt", "日本語.txt", "😀.txt", ".gitignore"] {
            assert!(validate_file_name(name).is_ok(), "{name:?}");
        }
    }

    #[test]
    fn rejects_empty_name() {
        assert!(matches!(
            validate_file_name(""),
            Err(Error::InvalidInput(InputError::EmptyName))
        ));
    }

    #[test]
    fn rejects_invalid_names() {
        for name in ["a\0b.txt", ".", ".."] {
            assert!(
                matches!(
                    validate_file_name(name),
                    Err(Error::InvalidInput(InputError::InvalidFileName))
                ),
                "{name:?}"
            );
        }
    }
}
