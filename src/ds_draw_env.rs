//! Process-start draw configuration cache; does not cache trigger-file contents,
//! GL state, guest data, input bindings, or atomic live performance controls.
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    sync::OnceLock,
};

struct Snapshot {
    values: HashMap<OsString, OsString>,
}
impl Snapshot {
    fn from_pairs(values: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        // Windows environment names are ASCII case-insensitive. Normalize
        // once while capturing, keeping uppercase hot-path lookups allocation-free.
        Self {
            values: values
                .into_iter()
                .map(|(key, value)| {
                    #[cfg(windows)]
                    let key = key.to_ascii_uppercase();
                    (key, value)
                })
                .filter(|(key, _)| key.to_str().is_some_and(|v| v.starts_with("TOUCHHLE_DS_")))
                .collect(),
        }
    }
    fn var_os(&self, name: &str) -> Option<OsString> {
        self.values.get(OsStr::new(name)).cloned()
    }
    fn var(&self, name: &str) -> Result<String, std::env::VarError> {
        self.var_os(name)
            .ok_or(std::env::VarError::NotPresent)?
            .into_string()
            .map_err(std::env::VarError::NotUnicode)
    }
}
pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var("TOUCHHLE_DS_GL_ENV_CACHE").as_deref() == Ok("1");
        eprintln!(
            "[DSgl-draw-config] effective={}",
            if on { "snapshot" } else { "live-env" }
        );
        on
    })
}
static VALUES: OnceLock<Snapshot> = OnceLock::new();

/// Called after app identity/defaults/options are applied, before its Environment
/// is created. App-picker draws before this point keep live environment reads.
pub(crate) fn initialize_after_app_configuration() {
    if enabled() {
        VALUES.get_or_init(|| Snapshot::from_pairs(std::env::vars_os()));
        eprintln!("[DSgl-draw-config] initialized after app configuration");
    }
}
pub(crate) fn var(name: &str) -> Result<String, std::env::VarError> {
    if enabled() && name.starts_with("TOUCHHLE_DS_") {
        if let Some(values) = VALUES.get() {
            return values.var(name);
        }
    }
    std::env::var(name)
}
pub(crate) fn var_os(name: &str) -> Option<OsString> {
    if enabled() && name.starts_with("TOUCHHLE_DS_") {
        if let Some(values) = VALUES.get() {
            return values.var_os(name);
        }
    }
    std::env::var_os(name)
}

#[cfg(test)]
mod draw_env_tests {
    use super::*;
    #[test]
    fn draw_env_snapshot_preserves_absent_empty_zero_unicode_and_namespace() {
        let values = [
            ("TOUCHHLE_DS_EMPTY", ""),
            ("TOUCHHLE_DS_ZERO", "0"),
            ("TOUCHHLE_DS_UNICODE", "路径/纹理:2"),
            ("UNRELATED_SECRET", "outside"),
        ]
        .map(|(a, b)| (OsString::from(a), OsString::from(b)));
        let s = Snapshot::from_pairs(values);
        assert!(matches!(
            s.var("TOUCHHLE_DS_MISSING"),
            Err(std::env::VarError::NotPresent)
        ));
        assert_eq!(s.var("TOUCHHLE_DS_EMPTY").unwrap(), "");
        assert_eq!(s.var_os("TOUCHHLE_DS_EMPTY"), Some(OsString::new()));
        assert_eq!(s.var("TOUCHHLE_DS_ZERO").unwrap(), "0");
        assert!(s.var_os("TOUCHHLE_DS_ZERO").is_some());
        assert_eq!(s.var("TOUCHHLE_DS_UNICODE").unwrap(), "路径/纹理:2");
        assert!(s.var_os("UNRELATED_SECRET").is_none());
    }
    #[cfg(windows)]
    #[test]
    fn draw_env_snapshot_preserves_windows_case_insensitive_configuration_names() {
        let s = Snapshot::from_pairs([(
            OsString::from("touchHLE_ds_miXeD"),
            OsString::from("enabled"),
        )]);
        assert_eq!(s.var("TOUCHHLE_DS_MIXED").unwrap(), "enabled");
    }
    #[cfg(windows)]
    #[test]
    fn draw_env_snapshot_preserves_nonunicode_presence_and_error_payload() {
        use std::os::windows::ffi::OsStringExt;
        let invalid = OsString::from_wide(&[0xd800]);
        let s = Snapshot::from_pairs([(OsString::from("TOUCHHLE_DS_INVALID"), invalid.clone())]);
        assert_eq!(s.var_os("TOUCHHLE_DS_INVALID"), Some(invalid.clone()));
        match s.var("TOUCHHLE_DS_INVALID") {
            Err(std::env::VarError::NotUnicode(value)) => assert_eq!(value, invalid),
            other => panic!("Wrong result: {other:?}"),
        }
    }
    #[test]
    fn draw_env_snapshot_owns_initial_values_without_reading_later_mutation() {
        let mut values = vec![(OsString::from("TOUCHHLE_DS_KEY"), OsString::from("initial"))];
        let s = Snapshot::from_pairs(values.clone());
        values[0].1 = OsString::from("changed");
        assert_eq!(s.var("TOUCHHLE_DS_KEY").unwrap(), "initial");
        assert_eq!(
            Snapshot::from_pairs(values).var("TOUCHHLE_DS_KEY").unwrap(),
            "changed"
        );
    }
}
