//! Synchronous optional trace output. Formatting follows all output gates.
use std::{fmt, fs::OpenOptions, io::Write, path::Path};
use std::sync::atomic::{AtomicU8, Ordering::{Acquire, Release, AcqRel}};

// Changed only by the explicit private diagnostic inbox. Normal mode is lazy.
// Bit0=eager replay, bit1=one pending proof at the actual formatting path.
static PRIVATE_PROBE_STATE: AtomicU8 = AtomicU8::new(0);
pub(crate) fn set_eager_for_private_probe(eager: bool) {
    PRIVATE_PROBE_STATE.store((eager as u8) | 2, Release);
}

#[derive(Clone, Copy)]
pub(crate) enum Message<'a> {
    Formatted(fmt::Arguments<'a>),
    Literal(&'a str),
}
impl<'a> From<fmt::Arguments<'a>> for Message<'a> {
    fn from(value: fmt::Arguments<'a>) -> Self { Self::Formatted(value) }
}
impl<'a> From<&'a str> for Message<'a> {
    fn from(value: &'a str) -> Self { Self::Literal(value) }
}

pub(crate) fn append_if_configured<'a>(
    path: Option<&Path>,
    trigger: Option<&Path>,
    message: impl Into<Message<'a>>,
) {
    let state = PRIVATE_PROBE_STATE.load(Acquire);
    append_in_mode(state & 1 != 0, path, trigger, message.into(), (state & 2 != 0).then_some(state));
}
fn append_in_mode(
    eager: bool, path: Option<&Path>, trigger: Option<&Path>, message: Message<'_>, proof: Option<u8>,
) {
    // Replay only former format! callers. Plain literal callers did not allocate
    // before their gates in the original implementation and must remain plain.
    let eager_message = match (eager, message) {
        (true, Message::Formatted(args)) => Some(args.to_string()),
        _ => None,
    };
    if let Some(state) = proof.filter(|_| matches!(message, Message::Formatted(_))) {
        // A later request must not be acknowledged by an older in-flight call.
        if PRIVATE_PROBE_STATE.compare_exchange(state, state & !2, AcqRel, Acquire).is_ok() {
            eprintln!("[DSmatrix-format] effective mode={} formatted_before_gate={} output_configured={}",
                if eager {"eager"} else {"lazy"}, eager_message.is_some(), path.is_some());
        }
    }
    let Some(path) = path else { return; };
    // This marker can change while the process runs. Do not cache its state.
    if trigger.is_some_and(|path| !path.exists()) { return; }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        // Preserve the original single formatted-line write behavior when ON;
        // disabled, gated and failed-output paths allocate no message String.
        match (eager_message, message) {
            (Some(message), _) => { let _ = writeln!(file, "{message}"); }
            (None, Message::Formatted(args)) => { let message = args.to_string(); let _ = writeln!(file, "{message}"); }
            (None, Message::Literal(message)) => { let _ = writeln!(file, "{message}"); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, path::PathBuf, sync::atomic::{AtomicU64, Ordering::Relaxed}};

    struct Observed<'a>(&'a Cell<u32>);
    impl fmt::Debug for Observed<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.set(self.0.get()+1);
            write!(f, "matrix #{} 中文", self.0.get())
        }
    }
    fn directory() -> PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("r15-lazy-matrix-trace-{}-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
            SEQUENCE.fetch_add(1, Relaxed)));
        std::fs::create_dir(&path).unwrap();
        path
    }
    #[test]
    fn unconfigured_output_does_not_evaluate_debug_formatting() {
        let calls = Cell::new(0);
        append_if_configured(None, None, format_args!("LoadMatrixf {:?}", Observed(&calls)));
        assert_eq!(calls.get(), 0);
    }
    #[test]
    fn absent_trigger_does_not_format_or_create_output() {
        let dir = directory(); let output = dir.join("matrix.log"); let marker = dir.join("trigger");
        let calls = Cell::new(0);
        append_if_configured(Some(&output), Some(&marker), format_args!("{:?}", Observed(&calls)));
        assert_eq!(calls.get(), 0); assert!(!output.exists());
    }
    #[test]
    fn failed_output_open_does_not_evaluate_debug_formatting() {
        let dir = directory(); let calls = Cell::new(0);
        append_if_configured(Some(&dir), None, format_args!("{:?}", Observed(&calls)));
        assert_eq!(calls.get(), 0);
    }
    #[test]
    fn enabled_output_appends_same_lines_and_observes_live_trigger_changes() {
        let dir = directory(); let output = dir.join("matrix.log"); let marker = dir.join("trigger");
        let calls = Cell::new(0);
        std::fs::write(&marker, b"1").unwrap();
        append_if_configured(Some(&output), Some(&marker), format_args!("LoadMatrixf {:?}", Observed(&calls)));
        std::fs::remove_file(&marker).unwrap();
        append_if_configured(Some(&output), Some(&marker), format_args!("ignored {:?}", Observed(&calls)));
        assert_eq!(calls.get(), 1);
        std::fs::write(&marker, b"1").unwrap();
        append_if_configured(Some(&output), Some(&marker), format_args!("MultMatrixf {:?}", Observed(&calls)));
        assert_eq!(calls.get(), 2);
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "LoadMatrixf matrix #1 中文\nMultMatrixf matrix #2 中文\n");
    }
    #[test]
    fn explicit_eager_probe_replays_formatting_before_absent_output_gate() {
        let calls = Cell::new(0);
        append_in_mode(true, None, None, format_args!("{:?}", Observed(&calls)).into(), None);
        assert_eq!(calls.get(), 1);
        append_in_mode(false, None, None, format_args!("{:?}", Observed(&calls)).into(), None);
        assert_eq!(calls.get(), 1);
    }
    #[test]
    fn eager_probe_formats_once_and_both_modes_preserve_literal_and_array_output() {
        let dir = directory(); let eager = dir.join("eager.log"); let lazy = dir.join("lazy.log");
        let matrix = [0.0f32, -0.0, 1.0, -2.5, 1.25e-6, 1000.0, 0.5, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        for (mode, path) in [(true, &eager), (false, &lazy)] {
            let calls = Cell::new(0);
            append_in_mode(mode, Some(path), None, format_args!("{:?}", Observed(&calls)).into(), None);
            assert_eq!(calls.get(), 1);
            append_in_mode(mode, Some(path), None, Message::Literal("LoadIdentity"), None);
            append_in_mode(mode, Some(path), None, format_args!("LoadMatrixf {:?}", matrix).into(), None);
        }
        assert_eq!(std::fs::read(&eager).unwrap(), std::fs::read(&lazy).unwrap());
    }
}
