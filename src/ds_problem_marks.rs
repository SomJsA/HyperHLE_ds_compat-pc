//! General user problem bookmarks, independent of automatic incident merging.
//! Explicit F7 performs one diagnostic dump stall; writer owns PNG/JSON/IO. No native pause or key trace.
use super::*;
use std::sync::Mutex;
const MAX_PENDING: usize = 16;
static SERIAL: AtomicU64 = AtomicU64::new(1);
static VISIBLE: AtomicBool = AtomicBool::new(false);
static CAPTURE: AtomicBool = AtomicBool::new(false);
static DUMP_DONE: AtomicU64 = AtomicU64::new(0);
static IN_FLIGHT: AtomicU64 = AtomicU64::new(0);
fn capture_ready(in_flight: u64, completed: u64) -> bool {
    in_flight == 0 || completed >= in_flight
}
fn captures() -> &'static Mutex<VecDeque<u64>> {
    static IDS: OnceLock<Mutex<VecDeque<u64>>> = OnceLock::new();
    IDS.get_or_init(|| Mutex::new(VecDeque::new()))
}
struct Notice {
    id: u64,
    status: u8,
    until: Instant,
}
fn notice_slot() -> &'static Mutex<Option<Notice>> {
    static SLOT: OnceLock<Mutex<Option<Notice>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}
fn notice(id: u64, status: u8) {
    if let Ok(mut slot) = notice_slot().lock() {
        if slot.as_ref().is_some_and(|old| old.id > id) {
            return;
        }
        *slot = Some(Notice {
            id,
            status,
            until: Instant::now() + Duration::from_secs(6),
        });
        VISIBLE.store(true, Relaxed);
    }
}
pub(super) fn text() -> Option<String> {
    if !VISIBLE.load(Relaxed) {
        return None;
    }
    let slot = notice_slot().lock().ok()?;
    let n = slot.as_ref()?;
    if Instant::now() >= n.until {
        VISIBLE.store(false, Relaxed);
        return None;
    }
    let text = match n.status {
        1 => format!("问题 #{:03}：正在保存问题记录", n.id),
        2 => format!("问题 #{:03}：已保存", n.id),
        3 => format!("问题 #{:03}：记录失败，请查看诊断状态", n.id),
        5 => format!("问题 #{:03}：截图与前段已记录", n.id),
        _ => "问题记录失败：诊断未启动或已停止".to_owned(),
    };
    Some(text)
}
pub(super) fn backend_failed() {
    let id = notice_slot()
        .lock()
        .ok()
        .and_then(|s| s.as_ref().map(|n| n.id))
        .unwrap_or(0);
    notice(id, 3);
}
pub(super) struct Problem {
    pub(super) id: u64,
    at: u64,
    frame: u64,
    source: &'static str,
    note: String,
}
impl Problem {
    pub(super) fn new(at: u64, source: &'static str, note: String) -> Self {
        Self {
            id: SERIAL.fetch_add(1, Relaxed),
            at,
            frame: crate::window::DS_FRAME_COUNTER.load(Relaxed),
            source,
            note,
        }
    }
    fn wants_picture(&self) -> bool {
        self.source == "F7_or_explicit_issue_command"
    }
    fn extra(&self) -> Value {
        json!({"issue_number":self.id,"source":self.source,"native_present_at_request":self.frame,"note":self.note,
        "screenshot_explicitly_requested":self.wants_picture(),"scope":"general gameplay problem; QTE is optional context; only explicit F7 requests one game framebuffer PNG; no ordinary input trace"})
    }
}
pub(super) fn request() {
    let id = SERIAL.fetch_add(1, Relaxed);
    let Some(c) = problem_client()
        .filter(|_| LEVEL.load(Relaxed) < 3)
    else {
        notice(id, 4);
        echo!("[DSissue] #{:03} failed: diagnostics unavailable", id);
        return;
    };
    let Ok(mut ids) = captures().lock() else {
        notice(id, 3);
        return;
    };
    if ids.len() >= 4 {
        notice(id, 3);
        echo!("[DSissue] #{:03} failed: bounded capture queue full", id);
        return;
    }
    let p = Problem {
        id,
        at: now(c),
        frame: crate::window::DS_FRAME_COUNTER.load(Relaxed),
        source: "F7_or_explicit_issue_command",
        note: String::new(),
    };
    notice(id, 1);
    if !send(c, Message::Problem(p)) {
        notice(id, 3);
        echo!("[DSissue] #{:03} failed: diagnostics closing/full/disconnected", id);
    } else {
        ids.push_back(id);
        CAPTURE.store(true, Relaxed);
        event(
            "diagnostic_problem_requested",
            [id, crate::window::DS_FRAME_COUNTER.load(Relaxed), 1, 0],
        );
        echo!("[DSissue] #{:03} queued; explicit diagnostic dump + one game PNG; auto continue, no native pause",id);
    }
}
pub(super) struct Picture {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub frame: u64,
    pub at: u64,
    pub readback_us: u64,
    pub rgba: Vec<u8>,
    pub failure: Option<&'static str>,
}
fn picture_shape(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return None;
    }
    let n = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    (n <= 128 * 1024 * 1024).then_some(n)
}
pub(super) unsafe fn capture(gles: &mut dyn crate::gles::GLES, viewport: (u32, u32, u32, u32)) {
    use crate::gles::gles11_raw as gl;
    if !CAPTURE.load(Relaxed) {
        return;
    }
    if !capture_ready(IN_FLIGHT.load(Relaxed), DUMP_DONE.load(Relaxed)) {
        return;
    }
    if !CAPTURE.swap(false, Relaxed) {
        return;
    }
    let Some(id) = captures().lock().ok().and_then(|mut ids| {
        let id = ids.pop_front();
        if !ids.is_empty() {
            CAPTURE.store(true, Relaxed);
        }
        id
    }) else {
        return;
    };
    let Some(c) = problem_client() else {
        notice(id, 3);
        return;
    };
    let Some(permit) = c.delivery.enter() else {
        notice(id, 3);
        DUMP_DONE.fetch_max(id, Relaxed);
        return;
    };
    IN_FLIGHT.store(id, Relaxed);
    let (width, height) = (viewport.2, viewport.3);
    let start = Instant::now();
    let (rgba, failure) = if let Some(n) = picture_shape(width, height) {
        let mut rgba = vec![0xa5; n];
        let mut pack = 4;
        gles.GetIntegerv(gl::PACK_ALIGNMENT, &mut pack);
        gles.PixelStorei(gl::PACK_ALIGNMENT, 1);
        gles.ReadPixels(
            viewport.0 as _,
            viewport.1 as _,
            width as _,
            height as _,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            rgba.as_mut_ptr().cast(),
        );
        gles.PixelStorei(gl::PACK_ALIGNMENT, pack);
        let untouched = rgba.iter().all(|v| *v == 0xa5);
        (rgba, untouched.then_some("readback_buffer_unchanged"))
    } else {
        (Vec::new(), Some("capture_shape_limit"))
    };
    let frame = crate::window::DS_FRAME_COUNTER.load(Relaxed);
    let us = start.elapsed().as_micros().min(u64::MAX as u128) as u64;
    event(
        "explicit_issue_capture_readback",
        [id, width as u64, height as u64, us],
    );
    let picture = Picture {
        id,
        width,
        height,
        frame,
        at: now(c),
        readback_us: us,
        rgba,
        failure,
    };
    if !send_admitted(c, Message::Picture(picture), permit) {
        notice(id, 3);
        DUMP_DONE.fetch_max(id, Relaxed);
        return;
    }
    // Explicitly requested dump stall only. The worker saves pre-context then
    // PNG; no forced minimum delay and no wait for the later +3s context tail.
    // A slow writer cannot trap gameplay indefinitely.
    let wait = Instant::now();
    while DUMP_DONE.load(Relaxed) < id
        && wait.elapsed() < Duration::from_secs(2)
        && !c.delivery.ended.load(std::sync::atomic::Ordering::Acquire)
        && !c.delivery.closed()
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    event(
        "explicit_issue_dump_wait",
        [
            id,
            start.elapsed().as_micros().min(u64::MAX as u128) as u64,
            (DUMP_DONE.load(Relaxed) >= id) as u64,
            wait.elapsed().as_micros().min(u64::MAX as u128) as u64,
        ],
    );
}
fn save_picture(root: &Path, mut p: Picture) -> io::Result<Value> {
    if let Some(reason) = p.failure {
        return Ok(json!({"status":"failed","reason":reason}));
    }
    if picture_shape(p.width, p.height) != Some(p.rgba.len()) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "capture shape"));
    }
    let stride = p.width as usize * 4;
    // GL bottom-up -> top-down in place, no STB global flip setting.
    for y in 0..p.height as usize / 2 {
        let bottom = (p.height as usize - 1 - y) * stride;
        let (head, tail) = p.rgba.split_at_mut(bottom);
        head[y * stride..(y + 1) * stride].swap_with_slice(&mut tail[..stride]);
    }
    let image = crate::image::Image::from_pixel_vec(p.rgba, (p.width, p.height));
    let mut png = Vec::<u8>::new();
    let result = image.write_png_image(
        (&mut png as *mut Vec<u8>).cast(),
        p.width as i32,
        p.height as i32,
        image.pixels(),
        stride as i32,
    );
    if result == 0 || !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "PNG encode"));
    }
    let name = format!("issue-{:04}.png", p.id);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(&name))?;
    file.write_all(&png)?;
    file.flush()?;
    Ok(
        json!({"status":"saved","file":name,"width":p.width,"height":p.height,"bytes":png.len(),
        "mono_us":p.at,"last_observed_native_present_at_capture":p.frame,"readback_wall_us":p.readback_us,
        "source":"one explicitly requested game output viewport; captured before next swap, not the desktop; PNG encode/IO on background writer"}),
    )
}
fn filename(id: u64, before: bool) -> String {
    format!("issue-{id:04}{}.json", if before { "-before" } else { "" })
}

// No ACTIVE client, frame ring, periodic metrics or automatic incidents. This
// worker is created only by the first explicit F7 with automatic diagnostics off.
pub(super) fn manual_worker(
    root: PathBuf, rx: mpsc::Receiver<Message>, start: Instant, wall: u128,
    version: String, delivery: &Delivery,
) -> io::Result<()> {
    fs::create_dir_all(root.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput,"manual root"))?)?;
    fs::create_dir(&root)?; // A session must be new; never overwrite a prior dump.
    let mut identity=json!({"available":false});
    if let Ok(exe)=std::env::current_exe() {
        if let Some(game)=exe.parent() {
            let info=game.join("build-info.json");
            if fs::metadata(&info).is_ok_and(|m|m.is_file() && m.len()<=1024*1024) {
                if let Ok(bytes)=fs::read(&info) {
                    if let Ok(value)=serde_json::from_slice::<Value>(&bytes) {
                        identity=json!({"available":true,"implementation_commit":value["implementation_commit"],
                            "distribution_id":value["distribution_id"],"native_sha256":value["native_sha256"],
                            "release_label":value["release_label"],"diagnostic_map_sha256":value["diagnostic_map_sha256"]});
                        let map=game.join("diagnostic-map.json");
                        if fs::metadata(&map).is_ok_and(|m|m.is_file() && m.len()<=4*1024*1024) {
                            if let Ok(bytes)=fs::read(&map) {
                                use sha2::{Digest,Sha256};
                                let actual=format!("{:x}",Sha256::digest(&bytes));
                                if value["diagnostic_map_sha256"].as_str()==Some(actual.as_str()) {
                                    let mut out=OpenOptions::new().create_new(true).write(true).open(root.join("diagnostic-map.json"))?;
                                    out.write_all(&bytes)?;identity["diagnostic_map_copied"]=json!(true);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let mut settings=serde_json::Map::new();
    for key in ["TOUCHHLE_DS_GPU_PRESENT_COPY","TOUCHHLE_DS_GPU_TEXT_OVERLAY","TOUCHHLE_DS_MSAA",
        "TOUCHHLE_DS_ANISO","TOUCHHLE_DS_OUTPUT_WIDTH","TOUCHHLE_DS_OUTPUT_HEIGHT","TOUCHHLE_DS_VSYNC"] {
        settings.insert(key.into(),json!(std::env::var(key).ok()));
    }
    let scale=std::env::args().find_map(|s|s.strip_prefix("--scale-hack=").map(str::to_owned));
    write_new(&root.join("host.json"),&json!({"schema":1,"kind":"deadspace-play-diagnostics",
        "mode":"manual_f7_only","wall_unix_ms":wall,"automatic_diagnostics":false,"build":identity,
        "requested_settings":settings,"internal_scale_requested":scale,
        "history_available":false,"history_missing_reason":"automatic diagnostics disabled; no prior frames/events were collected",
        "privacy":{"automatic_screenshots":false,"explicit_F7_game_frame":true,"keystrokes":false,"saves":false,"automatic_upload":false}}))?;
    write_new(&root.join("session-start.json"),&json!({"schema":1,"pid":std::process::id(),"wall_unix_ms":wall,
        "runtime_version":version,"monotonic_origin_us":0,"mode":"manual_f7_only","automatic_history_enabled":false}))?;
    let mut pending=std::collections::BTreeMap::<u64,Problem>::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(message)=>{
                delivery.queued.fetch_sub(1,Relaxed);
                match message {
                    Message::Problem(p)=>{
                        if pending.len()>=MAX_PENDING {notice(p.id,3);continue;}
                        write_new(&root.join(filename(p.id,true)),&json!({"schema":1,"issue_number":p.id,
                            "mode":"manual_f7_only","request_mono_us":p.at,"native_present_at_request":p.frame,
                            "history_available":false,"history_missing_reason":"automatic diagnostics disabled; no before/after frame history",
                            "status":"waiting_explicit_screenshot","explicit_user_note":p.extra()}))?;
                        pending.insert(p.id,p);
                    }
                    Message::Picture(picture)=>{
                        let id=picture.id;
                        let Some(p)=pending.remove(&id) else {notice(id,3);DUMP_DONE.fetch_max(id,Relaxed);continue;};
                        let saved=save_picture(&root,picture).unwrap_or_else(|e|json!({"status":"failed","reason":format!("{:?}",e.kind())}));
                        let ok=saved["status"]=="saved";let at=start.elapsed().as_micros() as u64;
                        let value=json!({"schema":1,"issue_number":id,"mode":"manual_f7_only",
                            "request_mono_us":p.at,"dumped_at_us":at,"native_present_at_request":p.frame,
                            "history_available":false,"history_missing_reason":"automatic diagnostics disabled; no before/after frame history",
                            "status":if ok{"saved_manual_capture"}else{"manual_capture_failed"},"picture":saved,
                            "explicit_user_note":p.extra()});
                        write_new(&root.join(filename(id,false)),&value)?;
                        replace_json(&root.join("issue-status.json"),&json!({"schema":1,"issue_number":id,
                            "status":value["status"],"file":filename(id,false),"picture":value["picture"]}))?;
                        notice(id,if ok{2}else{3});DUMP_DONE.fetch_max(id,Relaxed);
                        echo!("[DSissue] #{id:03} saved manual capture picture_ok={ok}; automatic diagnostics remain off");
                    }
                    _=>{} // Manual clients never record unsolicited frame/events.
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout)=>{}
            Err(mpsc::RecvTimeoutError::Disconnected)=>delivery.close(),
        }
        if delivery.closed() && (delivery.drained() || delivery.timed_out()) {
            for (id,p) in pending {
                write_new(&root.join(filename(id,false)),&json!({"schema":1,"issue_number":id,"mode":"manual_f7_only",
                    "status":"manual_screenshot_missing_at_exit","history_available":false,"native_present_at_request":p.frame}))?;
                notice(id,3);DUMP_DONE.fetch_max(id,Relaxed);
            }
            write_new(&root.join("session-end.json"),&json!({"schema":1,"mode":"manual_f7_only",
                "normal_exit_observed":!BAD_END.load(Relaxed),"shutdown_queue_drained":delivery.drained(),
                "shutdown_drain_timed_out":!delivery.drained(),"shutdown_senders_inflight":delivery.inflight(),
                "queued_tail_not_persisted":delivery.queued.load(Relaxed),"writer_error":null,
                "shutdown_rejected_after_close":delivery.rejected()}))?;
            return Ok(());
        }
    }
}
#[derive(Default)]
pub(super) struct Jobs {
    pending: VecDeque<Problem>,
    pictures: std::collections::BTreeMap<u64, Value>,
}
impl Jobs {
    pub(super) fn accept(
        &mut self,
        p: Problem,
        t: u64,
        root: &Path,
        history: &mut History,
        sink: &mut Sink,
    ) -> io::Result<()> {
        if self.pending.len() >= MAX_PENDING {
            notice(p.id, 3);
            echo!("[DSissue] #{:03} failed: bounded pending queue full", p.id);
            return Ok(());
        }
        notice(p.id, 1);
        let event = json!({"type":"manual_problem_mark","mono_us":p.at,"issue_number":p.id,"source":p.source,"note":p.note,"native_present_at_request":p.frame});
        history.event(event.clone());
        sink.json(&event)?;
        let mut initial = history.dump(p.at, t, "explicit_general_problem_mark", &p.extra());
        initial["issue_number"] = json!(p.id);
        initial["status"] = json!("before_window_saved_waiting_post_window");
        if let Err(e) = write_new(&root.join(filename(p.id, true)), &initial) {
            notice(p.id, 3);
            return Err(e);
        }
        self.pending.push_back(p);
        Ok(())
    }
    pub(super) fn picture(&mut self, picture: Picture, root: &Path) {
        let id = picture.id;
        if !self.pending.iter().any(|p| p.id == id) {
            DUMP_DONE.fetch_max(id, Relaxed);
            notice(id, 3);
            return;
        }
        let value = match save_picture(root, picture) {
            Ok(v) => v,
            Err(e) => json!({"status":"failed","reason":format!("{:?}",e.kind())}),
        };
        if value["status"] != "saved" {
            notice(id, 3);
        }
        if value["status"] == "saved" {
            notice(id, 5);
        }
        self.pictures.insert(id, value);
        DUMP_DONE.fetch_max(id, Relaxed);
    }
    pub(super) fn tick(
        &mut self,
        t: u64,
        ending: bool,
        root: &Path,
        history: &History,
    ) -> io::Result<()> {
        while self.pending.front().is_some_and(|p| {
            ending
                || (t >= p.at.saturating_add(AFTER_US)
                    && (!p.wants_picture()
                        || self.pictures.contains_key(&p.id)
                        || t >= p.at.saturating_add(10_000_000)))
        }) {
            let p = self.pending.pop_front().unwrap();
            let mut value = history.dump(p.at, t, "explicit_general_problem_mark", &p.extra());
            value["issue_number"] = json!(p.id);
            let picture = self.pictures.remove(&p.id).unwrap_or_else(
                || json!({"status":if p.wants_picture(){"missing"}else{"not_requested"}}),
            );
            let picture_ok = !p.wants_picture() || picture["status"] == "saved";
            value["picture"] = picture;
            value["status"] = json!(if !picture_ok {
                "saved_diagnostics_screenshot_failed_or_missing"
            } else if t < p.at.saturating_add(AFTER_US) {
                "saved_partial_post_window_at_exit"
            } else {
                "saved"
            });
            let name = filename(p.id, false);
            if let Err(e) = write_new(&root.join(&name), &value) {
                notice(p.id, 3);
                return Err(e);
            }
            notice(p.id, if picture_ok { 2 } else { 3 });
            echo!(
                "[DSissue] #{:03} saved {} picture_ok={} actual_until_us={}",
                p.id,
                name,
                picture_ok,
                t
            );
            replace_json(
                &root.join("issue-status.json"),
                &json!({"schema":1,"issue_number":p.id,"status":value["status"],"file":name,"mono_us":t,"picture":value["picture"]}),
            )?;
        }
        Ok(())
    }
    pub(super) fn len(&self) -> usize {
        self.pending.len()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_worker_saves_only_explicit_captures_and_stays_idle() {
        let root=temp().join("session-manual-test");
        let start=Instant::now();let (tx,rx)=mpsc::sync_channel(16);
        let delivery=Arc::new(Delivery::default());let writer_delivery=delivery.clone();let writer_root=root.clone();
        let writer=std::thread::spawn(move||manual_worker(writer_root,rx,start,0,"manual-test".into(),&writer_delivery));
        let c=Client{tx,start,delivery};
        let p=Problem::new(now(&c),"F7_or_explicit_issue_command",String::new());let id=p.id;
        assert!(send(&c,Message::Problem(p)));
        assert!(send(&c,Message::Frame(Frame{t:1,n:1,dt:100_000,phase:4,state:0,hud:0})));
        assert!(send(&c,Message::Event(1,"unsolicited_event",[0;4],1)));
        assert!(send(&c,Message::Picture(Picture{id,width:2,height:2,frame:7,at:now(&c),readback_us:2,
            rgba:vec![255u8;16],failure:None})));
        let deadline=Instant::now()+Duration::from_secs(5);
        while !root.join(filename(id,false)).exists() && Instant::now()<deadline {std::thread::sleep(Duration::from_millis(10));}
        let value:Value=serde_json::from_slice(&fs::read(root.join(filename(id,false))).unwrap()).unwrap();
        assert_eq!(value["status"],"saved_manual_capture");assert_eq!(value["history_available"],false);
        assert_eq!(value["picture"]["status"],"saved");
        assert!(fs::read(root.join(format!("issue-{id:04}.png"))).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
        let inventory=||{
            let mut files:Vec<_>=fs::read_dir(&root).unwrap().map(|p|{let p=p.unwrap().path();(p.file_name().unwrap().to_os_string(),fs::read(&p).unwrap())}).collect();
            files.sort_by(|a,b|a.0.cmp(&b.0));files
        };
        let before=inventory();std::thread::sleep(Duration::from_millis(150));assert_eq!(inventory(),before,"idle manual writer changed files");
        assert!(!root.join("health.json").exists());
        assert!(before.iter().all(|(n,_)|!n.to_string_lossy().starts_with("frames-") && !n.to_string_lossy().starts_with("timeline-")));
        c.delivery.close();writer.join().unwrap().unwrap();
        let end:Value=serde_json::from_slice(&fs::read(root.join("session-end.json")).unwrap()).unwrap();
        assert_eq!(end["shutdown_queue_drained"],true);assert_eq!(end["queued_tail_not_persisted"],0);
        assert!(ACTIVE.get().and_then(Option::as_ref).is_none(),"manual writer enabled ordinary collectors");
    }
    fn temp() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("ds-problem-mark-{}-{n}", std::process::id()));
        fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn problem_mark_general_numbered_files_keep_history_and_partial_exit_without_overwrite() {
        let root = temp();
        let mut sink = Sink::new(root.clone()).unwrap();
        let mut h = History::new();
        for t in [10_000_000, 39_000_000, 40_000_000, 43_000_000] {
            h.frame(Frame {
                t,
                n: t / 1000,
                dt: 16667,
                phase: 4,
                state: 0,
                hud: 0,
            });
        }
        h.event(json!({"type":"native_event","mono_us":39_000_000,"code":"native_qte_stage","values":[9,5,123,0]}));
        let mut jobs = Jobs::default();
        jobs.accept(
            Problem {
                id: 7,
                at: 40_000_000,
                frame: 40,
                source: "unit_general",
                note: "elevator stutter".into(),
            },
            40_000_000,
            &root,
            &mut h,
            &mut sink,
        )
        .unwrap();
        assert!(root.join("issue-0007-before.json").is_file());
        assert!(!root.join("issue-0007.json").exists());
        jobs.tick(42_999_999, false, &root, &h).unwrap();
        assert_eq!(jobs.len(), 1);
        jobs.tick(43_000_000, false, &root, &h).unwrap();
        let v: Value =
            serde_json::from_slice(&fs::read(root.join("issue-0007.json")).unwrap()).unwrap();
        assert_eq!(v["issue_number"], 7);
        assert_eq!(v["frames"].as_array().unwrap().len(), 4);
        assert_eq!(v["explicit_user_note"]["note"], "elevator stutter");
        assert!(!v["events"].as_array().unwrap().is_empty());
        assert!(write_new(&root.join("issue-0007.json"), &Value::Null).is_err());
        jobs.accept(
            Problem {
                id: 8,
                at: 44_000_000,
                frame: 44,
                source: "unit_general",
                note: String::new(),
            },
            44_000_000,
            &root,
            &mut h,
            &mut sink,
        )
        .unwrap();
        jobs.tick(44_100_000, true, &root, &h).unwrap();
        let v: Value =
            serde_json::from_slice(&fs::read(root.join("issue-0008.json")).unwrap()).unwrap();
        assert_eq!(v["status"], "saved_partial_post_window_at_exit");
        // Preserve explicit tiny TEMP fixture for diagnosis; no recursive delete.
    }
    #[test]
    fn problem_mark_explicit_png_has_correct_orientation_and_rejects_duplicate_or_large_shape() {
        let root = temp();
        let bottom = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let p = || Picture {
            id: 9,
            width: 2,
            height: 2,
            frame: 10,
            at: 11,
            readback_us: 12,
            rgba: bottom.clone(),
            failure: None,
        };
        let value = save_picture(&root, p()).unwrap();
        assert_eq!(value["status"], "saved");
        let png = fs::read(root.join("issue-0009.png")).unwrap();
        let image = crate::image::Image::from_bytes(&png).unwrap();
        assert_eq!(image.dimensions(), (2, 2));
        assert_eq!(&image.pixels()[..8], &bottom[8..]);
        assert_eq!(&image.pixels()[8..], &bottom[..8]);
        assert!(save_picture(&root, p()).is_err());
        assert_eq!(fs::read(root.join("issue-0009.png")).unwrap(), png);
        assert!(picture_shape(0, 1).is_none());
        assert!(picture_shape(16384, 16384).is_none());
        assert_eq!(picture_shape(3840, 2160), Some(33177600));
    }
    #[test]
    fn problem_mark_storage_conflict_fails_without_overwriting_existing_path() {
        let root = temp();
        let mut sink = Sink::new(root.clone()).unwrap();
        let mut h = History::new();
        let mut jobs = Jobs::default();
        let block = root.join("issue-0001-before.json");
        fs::create_dir(&block).unwrap();
        assert!(jobs
            .accept(
                Problem {
                    id: 1,
                    at: 1,
                    frame: 1,
                    source: "unit",
                    note: String::new()
                },
                1,
                &root,
                &mut h,
                &mut sink
            )
            .is_err());
        assert!(block.is_dir());
        assert_eq!(jobs.len(), 0);
        assert!(!root.join("issue-0001.json").exists());
    }
}
