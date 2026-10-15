use crate::{Error, FileNameErrorReason, InputError, Result};

pub(crate) fn validate_file_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::InvalidInput(InputError::EmptyName));
    }

    if name.contains('\0') {
        return Err(Error::InvalidInput(InputError::InvalidFileName {
            name: name.to_owned(),
            reason: FileNameErrorReason::InvalidCharacter('\0'),
        }));
    }

    for component in name.split('/') {
        if component.is_empty() || matches!(component, "." | "..") {
            return Err(Error::InvalidInput(InputError::InvalidFileName {
                name: name.to_owned(),
                reason: FileNameErrorReason::InvalidComponent(component.to_owned()),
            }));
        }
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
        assert!(matches!(
            validate_file_name("a\0b.txt"),
            Err(Error::InvalidInput(InputError::InvalidFileName {
                reason: FileNameErrorReason::InvalidCharacter('\0'),
                ..
            }))
        ));

        for name in [".", ".."] {
            assert!(
                matches!(
                    validate_file_name(name),
                    Err(Error::InvalidInput(InputError::InvalidFileName {
                        reason: FileNameErrorReason::InvalidComponent(_),
                        ..
                    }))
                ),
                "{name:?}"
            );
        }
    }

    #[test]
    fn rejects_invalid_relative_paths() {
        for name in [
            ".",
            "..",
            "a/../b.txt",
            "a/./b.txt",
            "a//b.txt",
            "/a.txt",
            "a/",
        ] {
            assert!(
                matches!(
                    validate_file_name(name),
                    Err(Error::InvalidInput(InputError::InvalidFileName {
                        reason: FileNameErrorReason::InvalidComponent(_),
                        ..
                    }))
                ),
                "{name:?}"
            );
        }
    }

    #[test]
    fn accepts_valid_relative_paths() {
        for name in [
            "file.txt",
            "folder/file.txt",
            "folder/sub/file.txt",
            ".gitignore",
        ] {
            assert!(validate_file_name(name).is_ok(), "{name:?}");
        }
    }
}
