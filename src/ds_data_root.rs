//! Optional host data root. Never reinterpret a relative path against the CWD.
use std::ffi::OsString;
use std::path::PathBuf;

pub fn configured_root(value: Option<OsString>) -> Result<Option<PathBuf>, &'static str> {
    let Some(value) = value else { return Ok(None) };
    if value.is_empty() {
        return Err("TOUCHHLE_USER_DATA_DIR must be an absolute, nonempty path");
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err("TOUCHHLE_USER_DATA_DIR must be an absolute, nonempty path");
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_preserves_portable_default() { assert_eq!(configured_root(None), Ok(None)); }
    #[test]
    fn rejects_empty_and_relative() {
        for path in ["", ".", "../saves", "saves"] {
            assert!(configured_root(Some(path.into())).is_err());
        }
    }
    #[test]
    fn accepts_unicode_absolute_without_creating_it() {
        let path = std::env::temp_dir().join("DeadSpace-文档-root");
        assert_eq!(configured_root(Some(path.clone().into_os_string())), Ok(Some(path)));
    }
}
