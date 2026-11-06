//! Opt-in, bounded diagnostic input for isolated VM runs. No shell commands.
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
pub enum Command {
    Touch(String),
    Action(String),
    Hud(String),
    Aim(bool),
    AimToggle,
    NativeStick(f32, f32),
    NativeLook(f32, f32),
    Ui(String),
    DirectionKey { slot: usize, down: bool },
    TestPadButton(String,bool),
    TestPadAxis(String,i16),
    Fire(u8),
    Release,
    Cancel,
    Mark,
    Probe,
    TestAssist(bool),
    ReportProblem,
    Checkpoint,
    PerfGpuCopy(bool),
    PerfTextOverlay(bool),
    PerfReadbackFinish(bool),
    PerfFpsLimit(u8),
    PerfTextureSync(String),
    PerfSkinHost(bool),
    PerfSkinInput(bool),
    PerfTransformHost(bool),
    PerfMatrixFormat(bool),
}

#[derive(Default)]
pub struct Inbox {
    last_sequence: u64,
    previous: String,
    path: Option<PathBuf>,
    next_read: Option<Instant>,
    initialized: bool,
}
impl Inbox {
    pub fn accept(&mut self, text: &str) -> Result<Option<(u64, Command)>, String> {
        if text == self.previous || text.trim().is_empty() {
            return Ok(None);
        }
        self.previous = text.to_string();
        if text.len() > 1024 || text.lines().count() != 1 {
            return Err("expected one bounded command line".into());
        }
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() < 2 {
            return Err("expected sequence and command".into());
        }
        let sequence = words[0].parse::<u64>().map_err(|_| "invalid sequence")?;
        if sequence <= self.last_sequence {
            return Err("stale sequence".into());
        }
        let args = &words[2..];
        let command = match words[1] {
            "pad" if args.len()==2 && ["down","up"].contains(&args[1]) => {
                if !["A","B","X","Y","LB","RB","START","BACK","LS","RS","UP","RIGHT","DOWN","LEFT"].contains(&args[0]) {return Err("unsupported test pad button".into());}
                Command::TestPadButton(args[0].into(),args[1]=="down")
            }
            "pad-axis" if args.len()==2 && ["LX","LY","RX","RY","LT","RT"].contains(&args[0]) => {
                let value=args[1].parse::<i16>().map_err(|_|"invalid test pad axis")?;
                Command::TestPadAxis(args[0].into(),value)
            }
            "key" if args.len()==2 && ["down","up"].contains(&args[0]) => {
                let slot=["W","A","S","D","UP","LEFT","DOWN","RIGHT","SPACE","E","G","B","LCTRL","RCTRL","Z"].iter().position(|key|*key==args[1]).ok_or("unsupported test key")?;
                Command::DirectionKey {slot,down:args[0]=="down"}
            }
            "tap" | "down" | "move" | "up" | "swipe" => {
                let n = if words[1] == "swipe" { 4 } else { 2 };
                if args.len() != n {
                    return Err("wrong touch argument count".into());
                }
                for (i, value) in args.iter().enumerate() {
                    let value = value.parse::<f32>().map_err(|_| "invalid coordinate")?;
                    let limit = if i % 2 == 0 { 480.0 } else { 320.0 };
                    if !value.is_finite() || !(0.0..=limit).contains(&value) {
                        return Err("coordinate outside landscape points".into());
                    }
                }
                Command::Touch(words[1..].join(" "))
            }
            "action"
                if args.len() == 1
                    && [
                        "interact", "kinesis", "stasis", "reload", "rotate", "combat",
                        "locator", "quick-turn",
                    ]
                    .contains(&args[0]) =>
            {
                Command::Action(args[0].into())
            }
            "hud" if args.len() == 1 && ["1", "2", "3", "4", "5", "P", "G", "RIG"].contains(&args[0]) => {
                Command::Hud(args[0].into())
            }
            "stick" | "look" if args.len() == 2 => {
                let x=args[0].parse::<f32>().map_err(|_| "invalid native vector")?;
                let y=args[1].parse::<f32>().map_err(|_| "invalid native vector")?;
                let bound=if words[1]=="stick" {1.0}else{1024.0};
                if !x.is_finite() || !y.is_finite() || x.abs()>bound || y.abs()>bound {return Err("native vector out of bounds".into());}
                if words[1]=="stick" {Command::NativeStick(x,y)}else{Command::NativeLook(x,y)}
            }
            "aim" if args == ["toggle"] => Command::AimToggle,
            "aim" if args == ["on"] => Command::Aim(true),
            "aim" if args == ["off"] => Command::Aim(false),
            "ui" if args.len() == 1
                && ["up", "down", "left", "right", "confirm", "cancel", "delete"].contains(&args[0]) =>
            {
                Command::Ui(args[0].into())
            }
            "fire" if args == ["primary"] => Command::Fire(1),
            "fire" if args == ["secondary"] => Command::Fire(2),
            "release" if args.is_empty() => Command::Release,
            "cancel" if args.is_empty() => Command::Cancel,
            "mark" if args.is_empty() => Command::Mark,
            "probe" if args.is_empty() => Command::Probe,
            "issue" if args.is_empty() => Command::ReportProblem,
            "assist" if args == ["on"] => Command::TestAssist(true),
            "assist" if args == ["off"] => Command::TestAssist(false),
            "checkpoint" if args.is_empty() => Command::Checkpoint,
            "perf" if args == ["gpu-copy", "on"] => Command::PerfGpuCopy(true),
            "perf" if args == ["gpu-copy", "off"] => Command::PerfGpuCopy(false),
            "perf" if args == ["matrix-format", "eager"] => Command::PerfMatrixFormat(true),
            "perf" if args == ["matrix-format", "lazy"] => Command::PerfMatrixFormat(false),
            "perf" if args == ["text-overlay", "gpu"] => Command::PerfTextOverlay(true),
            "perf" if args == ["text-overlay", "cpu"] => Command::PerfTextOverlay(false),
            "perf" if args == ["readback-finish", "on"] => Command::PerfReadbackFinish(true),
            "perf" if args == ["readback-finish", "off"] => Command::PerfReadbackFinish(false),
            "perf" if args == ["fps-limit", "off"] => Command::PerfFpsLimit(0),
            "perf" if args == ["fps-limit", "30"] => Command::PerfFpsLimit(30),
            "perf" if args == ["fps-limit", "60"] => Command::PerfFpsLimit(60),
            "perf" if args == ["fps-limit", "70"] => Command::PerfFpsLimit(70),
            "perf" if args.len() == 2
                && args[0] == "tex-sync"
                && ["finish", "flush", "off"].contains(&args[1]) =>
            {
                Command::PerfTextureSync(args[1].into())
            }
            "perf" if args.len() == 2
                && args[0] == "skin-host"
                && ["on", "off"].contains(&args[1]) =>
            {
                Command::PerfSkinHost(args[1] == "on")
            }
            "perf" if args.len() == 2
                && args[0] == "skin-input"
                && ["packed", "strided"].contains(&args[1]) =>
            {
                Command::PerfSkinInput(args[1] == "strided")
            }
            "perf" if args.len() == 2
                && args[0] == "transform-host"
                && ["on", "off"].contains(&args[1]) =>
            {
                Command::PerfTransformHost(args[1] == "on")
            }
            _ => return Err("unsupported diagnostic command".into()),
        };
        self.last_sequence = sequence;
        Ok(Some((sequence, command)))
    }

    /// Ignore a command already present at startup. The controller must publish
    /// a new sequence after receiving the ready log, avoiding stale replay.
    pub fn poll(&mut self) -> Result<Option<(u64, Command)>, String> {
        if !self.initialized {
            self.initialized = true;
            self.path = std::env::var_os("TOUCHHLE_DS_CONTROL_INBOX").map(Into::into);
            if let Some(path) = &self.path {
                let text = std::fs::File::open(path).and_then(|file| {
                    let mut text = String::new();
                    file.take(1025).read_to_string(&mut text)?;
                    Ok(text)
                });
                if let Ok(text) = text {
                    if text.len() <= 1024 {
                        self.last_sequence = text
                            .split_whitespace()
                            .next()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                        self.previous = text;
                    }
                }
            }
        }
        let Some(path) = &self.path else {
            return Ok(None);
        };
        if self.next_read.is_some_and(|at| Instant::now() < at) {
            return Ok(None);
        }
        self.next_read = Some(Instant::now() + Duration::from_millis(200));
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(_) => return Ok(None),
        };
        let mut text = String::new();
        file.take(1025)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        self.accept(&text)
    }
    pub fn configured(&self) -> bool {
        self.path.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sequence_prevents_replay_and_rejects_stale_writes() {
        let mut i = Inbox::default();
        assert_eq!(i.accept("1 mark").unwrap(), Some((1, Command::Mark)));
        assert_eq!(i.accept("1 mark").unwrap(), None);
        assert!(i.accept("1 probe").is_err());
        assert_eq!(i.accept("2 release").unwrap(), Some((2, Command::Release)));
    }
    #[test]
    fn malformed_batch_cannot_partially_inject_input() {
        let mut i = Inbox::default();
        for text in [
            "1 tap 2 3\n2 fire primary",
            "1 tap NaN 3",
            "1 tap 481 3",
            "1 swipe 2 3 4",
            "1 shell whoami",
        ] {
            assert!(i.accept(text).is_err());
        }
        assert_eq!(
            i.accept("1 tap 240 160").unwrap(),
            Some((1, Command::Touch("tap 240 160".into())))
        );
    }
    #[test]
    fn native_diagnostics_are_finite_bounded_and_atomic() {
        let mut i=Inbox::default();
        for text in ["1 stick 2 0","1 stick NaN 0","1 look inf 0","1 look 1025 0","1 look 0 0\n2 aim toggle"] {assert!(i.accept(text).is_err());}
        assert_eq!(i.accept("1 stick 0 -1").unwrap(),Some((1,Command::NativeStick(0.0,-1.0))));
        assert_eq!(i.accept("2 look 20 -10").unwrap(),Some((2,Command::NativeLook(20.0,-10.0))));
        assert_eq!(i.accept("3 aim toggle").unwrap(),Some((3,Command::AimToggle)));
        assert_eq!(i.accept("4 ui down").unwrap(),Some((4,Command::Ui("down".into()))));
        assert_eq!(i.accept("5 perf gpu-copy on").unwrap(),Some((5,Command::PerfGpuCopy(true))));
        assert_eq!(i.accept("6 perf tex-sync flush").unwrap(),Some((6,Command::PerfTextureSync("flush".into()))));
        assert_eq!(i.accept("7 perf skin-host on").unwrap(),Some((7,Command::PerfSkinHost(true))));
        assert_eq!(i.accept("8 perf skin-host off").unwrap(),Some((8,Command::PerfSkinHost(false))));
        assert!(i.accept("9 perf skin-host auto").is_err());
        assert!(i.accept("10 perf tex-sync turbo").is_err());
        assert_eq!(i.accept("11 perf text-overlay gpu").unwrap(),Some((11,Command::PerfTextOverlay(true))));
        assert_eq!(i.accept("12 perf text-overlay cpu").unwrap(),Some((12,Command::PerfTextOverlay(false))));
        assert!(i.accept("13 perf text-overlay auto").is_err());
        assert_eq!(i.accept("14 perf readback-finish on").unwrap(),Some((14,Command::PerfReadbackFinish(true))));
        assert_eq!(i.accept("15 perf readback-finish off").unwrap(),Some((15,Command::PerfReadbackFinish(false))));
        assert!(i.accept("16 perf readback-finish auto").is_err());
        assert_eq!(i.accept("17 perf fps-limit 60").unwrap(),Some((17,Command::PerfFpsLimit(60))));
        assert_eq!(i.accept("18 perf fps-limit off").unwrap(),Some((18,Command::PerfFpsLimit(0))));
        assert!(i.accept("19 perf fps-limit -1").is_err());
        assert!(i.accept("20 perf fps-limit nan").is_err());
        assert_eq!(i.accept("21 perf fps-limit 70").unwrap(),Some((21,Command::PerfFpsLimit(70))));
    }

    #[test]
    fn transform_toggle_is_explicit_and_does_not_replay_stale_input() {
        let mut i=Inbox::default();
        assert_eq!(i.accept("1 perf transform-host on").unwrap(),Some((1,Command::PerfTransformHost(true))));
        assert_eq!(i.accept("1 perf transform-host on").unwrap(),None);
        assert!(i.accept("2 perf transform-host auto").is_err());
        assert!(i.accept("2 perf transform-host on\n3 fire primary").is_err());
        assert_eq!(i.accept("2 perf transform-host off").unwrap(),Some((2,Command::PerfTransformHost(false))));
    }
    #[test]
    fn matrix_format_probe_is_explicit_sequence_checked_and_single_command() {
        let mut i = Inbox::default();
        assert_eq!(i.accept("1 perf matrix-format eager").unwrap(), Some((1,Command::PerfMatrixFormat(true))));
        assert_eq!(i.accept("1 perf matrix-format eager").unwrap(), None);
        assert!(i.accept("2 perf matrix-format auto").is_err());
        assert!(i.accept("2 perf matrix-format lazy\n3 fire primary").is_err());
        assert_eq!(i.accept("2 perf matrix-format lazy").unwrap(), Some((2,Command::PerfMatrixFormat(false))));
    }
    #[test]
    fn skin_input_mode_is_explicit_atomic_and_sequence_checked() {
        let mut i = Inbox::default();
        assert_eq!(i.accept("1 perf skin-input strided").unwrap(), Some((1,Command::PerfSkinInput(true))));
        assert_eq!(i.accept("1 perf skin-input strided").unwrap(), None);
        assert!(i.accept("1 perf skin-input packed").is_err());
        assert!(i.accept("2 perf skin-input auto").is_err());
        assert!(i.accept("2 perf skin-input packed\n3 fire primary").is_err());
        assert_eq!(i.accept("2 perf skin-input packed").unwrap(), Some((2,Command::PerfSkinInput(false))));
    }
    #[test]
    fn direction_diagnostics_are_whitelisted_and_do_not_accept_hotkeys() {
        let mut i=Inbox::default();
        for text in ["1 key down F8","1 key down ALT","1 key down W up A","1 key repeat A","1 key down A\n2 key up D"] {assert!(i.accept(text).is_err());}
        assert_eq!(i.accept("1 key down D").unwrap(),Some((1,Command::DirectionKey{slot:3,down:true})));
        assert_eq!(i.accept("2 key up LEFT").unwrap(),Some((2,Command::DirectionKey{slot:5,down:false})));
    }

    #[test]
    fn test_assist_commands_are_bounded_semantic_requests() {
        let mut i=Inbox::default();
        for text in ["1 assist write 0x258 100", "1 assist god 1", "1 assist on\n2 fire primary"] {assert!(i.accept(text).is_err());}
        assert_eq!(i.accept("1 assist on").unwrap(),Some((1,Command::TestAssist(true))));
        assert_eq!(i.accept("2 assist off").unwrap(),Some((2,Command::TestAssist(false))));
    }
    #[test]
    fn supported_semantics_have_no_unrestricted_action_codes() {
        let mut i = Inbox::default();
        for (n, cmd) in [
            "action reload",
            "hud P",
            "aim on",
            "fire secondary",
            "release",
            "cancel",
            "checkpoint",
        ]
        .iter()
        .enumerate()
        {
            assert!(i.accept(&format!("{} {}", n + 1, cmd)).unwrap().is_some());
        }
        assert!(i.accept("8 action 0xdeadbeef").is_err());
    }
}
