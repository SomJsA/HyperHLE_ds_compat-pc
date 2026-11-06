//! Real writer tests: only generated tiny TEMP data, never a game/IPA/save.
use super::*;
use std::sync::Barrier;
use std::thread::{self, JoinHandle};

fn fixture() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ds-shutdown-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    path
}
fn client() -> (Arc<Client>, mpsc::Receiver<Message>) {
    let (tx, rx) = mpsc::sync_channel(QUEUE);
    (
        Arc::new(Client {
            tx,
            start: Instant::now(),
            delivery: Arc::new(Delivery::default()),
        }),
        rx,
    )
}
fn run(root: &Path, c: &Arc<Client>, rx: mpsc::Receiver<Message>) -> JoinHandle<()> {
    COUNTERS.get_or_init(|| (0..PHASES).map(|_| Counter::new()).collect());
    let root = root.to_owned();
    let c = c.clone();
    thread::spawn(move || {
        worker(
            root,
            rx,
            c.start,
            0,
            "shutdown-regression".into(),
            &c.delivery,
        )
        .unwrap();
        c.delivery
            .ended
            .store(true, std::sync::atomic::Ordering::Release);
    })
}
fn read(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn end(root: &Path) -> Value {
    read(root.join("session-end.json"))
}
fn clean_end(root: &Path) {
    let e = end(root);
    assert_eq!(e["queued_tail_not_persisted"], 0);
    assert_eq!(e["shutdown_senders_inflight"], 0);
    assert_eq!(e["shutdown_queue_drained"], true);
    assert_eq!(e["shutdown_drain_timed_out"], false);
    assert_eq!(e["writer_error"], Value::Null);
}
fn events(root: &Path) -> Vec<Value> {
    let mut paths = fs::read_dir(root)
        .unwrap()
        .map(|v| v.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("timeline-")
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .flat_map(|p| {
            fs::read_to_string(p)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect::<Vec<_>>()
        })
        .collect()
}
fn pair(c: &Client) -> (problem_marks::Problem, problem_marks::Picture) {
    let p = problem_marks::Problem::new(now(c), "F7_or_explicit_issue_command", String::new());
    let picture = problem_marks::Picture {
        id: p.id,
        width: 2,
        height: 2,
        frame: 1,
        at: now(c),
        readback_us: 0,
        rgba: vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
        failure: None,
    };
    (p, picture)
}

#[test]
fn real_writer_drains_preaccepted_frame_event_feature_and_f7_pair_at_exit() {
    let root = fixture();
    let (c, rx) = client();
    for n in 0..64 {
        assert!(send(
            &c,
            Message::Frame(Frame {
                t: n,
                n,
                dt: 16667,
                phase: 4,
                state: 0,
                hud: 0
            })
        ));
        assert!(send(
            &c,
            Message::Event(n, "shutdown_tail", [n, 0, 0, 0], 0)
        ));
        assert!(send(
            &c,
            Message::Feature(n, "shutdown_feature", n.to_string(), 0)
        ));
    }
    let (p, picture) = pair(&c);
    let id = p.id;
    assert!(send(&c, Message::Problem(p)));
    assert!(send(&c, Message::Picture(picture)));
    c.delivery.close();
    assert!(!send(&c, Message::Event(0, "after_close", [0; 4], 0)));
    run(&root, &c, rx).join().unwrap();
    clean_end(&root);
    let rows = events(&root);
    assert_eq!(
        rows.iter().filter(|v| v["code"] == "shutdown_tail").count(),
        64
    );
    assert_eq!(
        rows.iter()
            .filter(|v| v["code"] == "shutdown_feature")
            .count(),
        64
    );
    assert!(!rows.iter().any(|v| v["code"] == "after_close"));
    assert_eq!(
        fs::read_to_string(root.join("frames-000000.csv"))
            .unwrap()
            .lines()
            .count(),
        65
    );
    let issue = read(root.join(format!("issue-{id:04}.json")));
    assert_eq!(issue["status"], "saved_partial_post_window_at_exit");
    assert_eq!(issue["picture"]["status"], "saved");
    assert_eq!(end(&root)["shutdown_rejected_after_close"], 1);
    assert!(root.join(format!("issue-{id:04}.png")).is_file());
}

#[test]
fn picture_admitted_before_close_can_finish_after_close_without_early_finalization() {
    let root = fixture();
    let (c, rx) = client();
    let (p, picture) = pair(&c);
    let id = p.id;
    assert!(send(&c, Message::Problem(p)));
    let permit = c.delivery.enter().unwrap();
    c.delivery.close();
    let worker = run(&root, &c, rx);
    thread::sleep(Duration::from_millis(10));
    assert!(!c.delivery.ended.load(std::sync::atomic::Ordering::Acquire));
    assert!(send_admitted(&c, Message::Picture(picture), permit));
    worker.join().unwrap();
    clean_end(&root);
    assert_eq!(
        read(root.join(format!("issue-{id:04}.json")))["picture"]["status"],
        "saved"
    );
}

#[test]
fn concurrent_producers_and_close_account_for_every_admitted_message() {
    let root = fixture();
    let (c, rx) = client();
    let worker = run(&root, &c, rx);
    let ready = Arc::new(Barrier::new(5));
    let release = Arc::new(Barrier::new(5));
    let mut producers = Vec::new();
    for n in 0..4 {
        let c = c.clone();
        let ready = ready.clone();
        let release = release.clone();
        producers.push(thread::spawn(move || {
            let permit = c.delivery.enter().unwrap();
            ready.wait();
            release.wait();
            assert!(send_admitted(
                &c,
                Message::Event(0, "admitted_before_close", [n, 0, 0, 0], n),
                permit
            ));
            for _ in 0..100 {
                assert!(!send(&c, Message::Event(0, "late", [n, 0, 0, 0], n)));
            }
        }));
    }
    ready.wait();
    c.delivery.close();
    assert_eq!(c.delivery.inflight(), 4);
    release.wait();
    for p in producers {
        p.join().unwrap();
    }
    worker.join().unwrap();
    clean_end(&root);
    assert_eq!(c.delivery.rejected(), 400);
    let rows = events(&root);
    assert_eq!(
        rows.iter()
            .filter(|v| v["code"] == "admitted_before_close")
            .count(),
        4
    );
    assert!(!rows.iter().any(|v| v["code"] == "late"));
}

#[test]
fn shutdown_deadline_discloses_stalled_sender_and_missing_picture_instead_of_success() {
    let root = fixture();
    let (c, rx) = client();
    let (p, _picture) = pair(&c);
    let id = p.id;
    assert!(send(&c, Message::Problem(p)));
    let permit = c.delivery.enter().unwrap();
    c.delivery.close();
    let worker = run(&root, &c, rx);
    worker.join().unwrap();
    let e = end(&root);
    assert_eq!(e["shutdown_queue_drained"], false);
    assert_eq!(e["shutdown_drain_timed_out"], true);
    assert_eq!(e["shutdown_senders_inflight"], 1);
    assert_eq!(
        read(root.join(format!("issue-{id:04}.json")))["status"],
        "saved_diagnostics_screenshot_failed_or_missing"
    );
    drop(permit);
    assert!(!send(&c, Message::Picture(pair(&c).1)));
    assert_eq!(c.delivery.queued.load(Relaxed), 0);
}

#[test]
fn main_exit_wait_is_bounded_even_without_writer_progress() {
    let state = Delivery::default();
    state.close();
    let before = Instant::now();
    assert!(!wait_for_writer(&state));
    let elapsed = before.elapsed();
    assert!(elapsed >= Duration::from_millis(190));
    assert!(elapsed < Duration::from_secs(1));
}
