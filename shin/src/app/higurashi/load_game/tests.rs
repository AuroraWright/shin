use super::*;
mod hex {
    pub fn encode(data: impl AsRef<[u8]>) -> String {
        use std::fmt::Write;
        let mut text = String::new();
        for byte in data.as_ref() {
            write!(&mut text, "{byte:02x}").unwrap();
        }
        text
    }
    pub fn decode(text: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
        assert_eq!(text.len() % 2, 0);
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16))
            .collect()
    }
}
use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn native(variable: &str, protocol: String) -> Vec<String> {
    let mut child = Command::new(std::env::var(variable).unwrap())
        .args([
            "higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(protocol.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}
struct Reference {
    status: u32,
    result: u32,
    decode: u32,
    starts: u32,
    loaded: u32,
    children: u32,
    kind: u32,
    selected: u32,
}
impl Reference {
    fn new() -> Self {
        Self {
            status: 0,
            result: 0,
            decode: 0,
            starts: 0,
            loaded: 0,
            children: 0,
            kind: 99,
            selected: 0,
        }
    }
    fn words(&self, model: &Model) -> Vec<u32> {
        vec![
            model.phase as u32,
            model.source as u32,
            self.starts,
            self.loaded,
            self.children,
            self.kind,
            self.selected,
            1,
            0,
            model.complete(self.children == 0) as u32,
        ]
    }
}
impl Services for Reference {
    fn start(&mut self, op: Operation) -> Result<()> {
        self.starts = match op {
            Operation::Read(_) => 1,
            Operation::TrialExists => 2,
        };
        Ok(())
    }
    fn status(&mut self, model: &mut Model) -> Result<u32> {
        model.result = self.result;
        model.decode_status = self.decode;
        Ok(self.status)
    }
    fn question(&mut self, kind: QuestionKind) -> Result<()> {
        self.kind = match kind {
            QuestionKind::Recovery(v) => v.min(3),
            QuestionKind::Inherit => 10,
            _ => panic!("desktop IO policy in native model"),
        };
        self.selected = kind.selected() as u32;
        self.children += 1;
        Ok(())
    }
    fn loaded(&mut self) -> Result<()> {
        self.loaded += 1;
        Ok(())
    }
}

#[test]
#[ignore = "original updated ctor/poll/recovery and inheritance builders/callbacks"]
fn higurashi_load_game_matches_native_owner_and_callbacks() {
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for status in 0..4 {
        for token in [false, true] {
            for result in [0, 0x202, 0x400202, 0x1000] {
                for decode in 0..4 {
                    for permission in 0..2 {
                        for delta in [0, 1, 120] {
                            protocol.push_str(&format!("N {}\n", token as u32));
                            let mut model = Model::new(token);
                            let mut reference = Reference::new();
                            for state in [status, 2] {
                                protocol.push_str(&format!(
                                    "U {state} {result} {decode} {permission} {delta}\n"
                                ));
                                reference.status = state;
                                reference.result = result;
                                reference.decode = decode;
                                reference.starts = 0;
                                model.poll(&mut reference).unwrap();
                                expected.push(("U", reference.words(&model)));
                            }
                            if reference.children != 0 {
                                let kind = if reference.kind == 10 {
                                    QuestionKind::Inherit
                                } else {
                                    QuestionKind::Recovery(decode)
                                };
                                // Include Yes,No,Cancel and notification dismissal; only final stage
                                // invokes the callback, including cancellation on recovery notices.
                                let button = match (owners / 3) % 4 {
                                    0 => 0,
                                    1 => 1,
                                    2 => -1,
                                    _ => -2,
                                };
                                for stage in [0, 1] {
                                    protocol.push_str(&format!("Q {stage} {button}\n"));
                                    model
                                        .callback(
                                            kind,
                                            question::Callback { stage, button },
                                            &mut reference,
                                        )
                                        .unwrap();
                                    expected
                                        .push(("Q", vec![model.phase as u32, reference.loaded]));
                                }
                            }
                            protocol.push_str("P\n");
                            reference.children = 0;
                            expected.push(("P", vec![model.complete(true) as u32]));
                            protocol.push_str("U 2 0 0 1 1\n");
                            reference.status = 2;
                            reference.result = 0;
                            reference.decode = 0;
                            reference.starts = 0;
                            model.poll(&mut reference).unwrap();
                            expected.push(("U", reference.words(&model)));
                            protocol.push_str("P\n");
                            expected.push(("P", vec![model.complete(true) as u32]));
                            owners += 1;
                        }
                    }
                }
            }
        }
    }
    let output = native("SHIN_HIGURASHI_NATIVE_LOAD_GAME", protocol);
    assert_eq!(output.len(), expected.len());
    for (i, (line, (op, words))) in output.iter().zip(&expected).enumerate() {
        let mut fields = line.split_whitespace();
        assert_eq!(fields.next(), Some(*op));
        assert_eq!(
            fields
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>(),
            *words,
            "native LoadGame observation{i}"
        );
    }
    eprintln!(
        "Higurashi LoadGame model:{owners} owners/{} observations/{} exact words pass",
        expected.len(),
        expected.iter().map(|(_, w)| w.len()).sum::<usize>()
    );
}

struct Memory {
    token: bool,
    bytes: Option<Vec<u8>>,
    exists: bool,
}
impl Storage for Memory {
    fn trial_token(&self) -> bool {
        self.token
    }
    fn read(&self, _: Source) -> Result<Option<Vec<u8>>> {
        Ok(self.bytes.clone())
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(self.exists)
    }
}
#[test]
#[ignore = "original full updated read/decode/CRC worker and trial-existence check;bounded in-memory SDK"]
fn higurashi_load_game_read_workers_match_native() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../reverse-engineering/higurashi/native-save-codec-golden.json"
    ))
    .unwrap();
    let good: Vec<Vec<u8>> = corpus["profiles"].as_array().unwrap()[..9]
        .iter()
        .map(|p| hex::decode(p["encrypted"].as_str().unwrap()).unwrap())
        .collect();
    let mut payloads: Vec<Option<Vec<u8>>> = good.iter().cloned().map(Some).collect();
    payloads.push(None);
    payloads.extend((0..4).map(|n| Some(vec![0; n])));
    let mut corrupt = good[1].clone();
    corrupt[12] ^= 128;
    payloads.push(Some(corrupt));
    let encrypt = |raw: &[u8]| {
        shin_core::format::save::SaveData::obfuscate_with_key(
            raw,
            shin_core::format::save::higurashi::GAME_KEY,
        )
    };
    payloads.push(Some(encrypt(&[0])));
    payloads.push(Some(encrypt(&[3])));
    payloads.push(Some(shin_core::format::save::SaveData::default().encode()));
    let defaults = hex::encode(SaveData::default().encode_raw().unwrap());
    let mut protocol = String::new();
    let mut expected = Vec::new();
    for source in [Source::Current, Source::Trial] {
        for bytes in &payloads {
            protocol.push_str(&format!(
                "W {} 0 {} {}\n",
                source as u32,
                bytes.is_none() as u32,
                bytes
                    .as_ref()
                    .filter(|b| !b.is_empty())
                    .map(hex::encode)
                    .unwrap_or("-".into())
            ));
            let output = run(
                &Memory {
                    token: true,
                    bytes: bytes.clone(),
                    exists: true,
                },
                Operation::Read(source),
            )
            .unwrap();
            expected.push((
                vec![
                    u32::from(output.data.is_some()),
                    output.decode_status,
                    output.result,
                    0,
                    if bytes.is_none() { 2 } else { 4 },
                    u32::from(bytes.is_some()),
                    1,
                ],
                hex::encode(output.data.unwrap_or_default().encode_raw().unwrap()),
            ));
        }
    }
    for source in [Source::Current, Source::Trial] {
        for failure in 1..=4 {
            protocol.push_str(&format!(
                "W {} {failure} 0 {}\n",
                source as u32,
                hex::encode(&good[1])
            ));
            expected.push((
                if failure == 2 {
                    vec![0, 1, 0x123456, 0, 2, 0, 1]
                } else {
                    vec![99, 0, 0x123456, 1, failure, 0, 0]
                },
                defaults.clone(),
            ));
        }
    }
    for exists in [false, true] {
        assert_eq!(
            run(
                &Memory {
                    token: true,
                    bytes: None,
                    exists
                },
                Operation::TrialExists
            )
            .unwrap()
            .result,
            if exists { 0 } else { MISSING }
        );
        protocol.push_str(&format!("T {}\n", exists as u32));
    }
    let output = native("SHIN_HIGURASHI_NATIVE_LOAD_GAME_READ", protocol);
    assert_eq!(output.len(), expected.len() + 2);
    for (line, (words, data)) in output.iter().zip(&expected) {
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields[0], "W");
        assert_eq!(
            fields[1..8]
                .iter()
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>(),
            *words
        );
        assert_eq!(fields[8], data);
    }
    assert_eq!(&output[expected.len()..], ["T 1 514", "T 1 0"]);
    eprintln!(
        "Higurashi LoadGame producers:{} native current/trial/CRC/version/missing/SDK-error/check cases pass",
        output.len()
    );
}

#[test]
fn higurashi_load_game_paths_keep_storage_failures_distinct_and_do_not_write() {
    let dir = std::env::temp_dir().join(format!(
        "higurashi-loadgame-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("SAVEDATA");
    let storage = Paths {
        current: Some(path.clone()),
        trial: Some(path.clone()),
    };
    assert!(storage.trial_token());
    assert!(!storage.trial_exists().unwrap());
    assert!(storage.read(Source::Current).unwrap().is_none());
    assert!(!path.exists());
    let mut data = SaveData::default();
    data.play_seconds = 93;
    data.extra_word = 0x81234567;
    let bytes = data.encode().unwrap();
    std::fs::write(&path, &bytes).unwrap();
    assert!(storage.trial_exists().unwrap());
    let loaded = run(&storage, Operation::Read(Source::Current))
        .unwrap()
        .data
        .unwrap();
    assert_eq!(loaded.encode().unwrap(), bytes);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    std::fs::write(&path, [1, 2, 3]).unwrap();
    assert_eq!(
        run(&storage, Operation::Read(Source::Current))
            .unwrap()
            .decode_status,
        1
    );
    assert_eq!(std::fs::read(&path).unwrap(), [1, 2, 3]);
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(storage.read(Source::Current).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn higurashi_load_game_producer_retirement_joins_blocked_storage() {
    use std::sync::{Condvar, Mutex as StdMutex, mpsc};
    struct Blocked {
        started: StdMutex<Option<mpsc::Sender<()>>>,
        gate: Arc<(StdMutex<bool>, Condvar)>,
    }
    impl Storage for Blocked {
        fn trial_token(&self) -> bool {
            false
        }
        fn trial_exists(&self) -> Result<bool> {
            Ok(false)
        }
        fn read(&self, _: Source) -> Result<Option<Vec<u8>>> {
            self.started
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(())
                .unwrap();
            let (lock, cv) = &*self.gate;
            let mut open = lock.lock().unwrap();
            while !*open {
                open = cv.wait(open).unwrap();
            }
            Ok(None)
        }
    }
    let (send, recv) = mpsc::channel();
    let gate = Arc::new((StdMutex::new(false), Condvar::new()));
    let worker = Worker::start(
        Arc::new(Blocked {
            started: StdMutex::new(Some(send)),
            gate: gate.clone(),
        }),
        Operation::Read(Source::Current),
    );
    let retirement = worker.retirement.clone();
    recv.recv_timeout(Duration::from_secs(5)).unwrap();
    let (send, recv) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        drop(worker);
        send.send(()).unwrap();
    });
    assert!(recv.recv_timeout(Duration::from_millis(20)).is_err());
    assert!(!retirement.is_finished());
    let (lock, cv) = &*gate;
    *lock.lock().unwrap() = true;
    cv.notify_all();
    recv.recv_timeout(Duration::from_secs(5)).unwrap();
    thread.join().unwrap();
    assert!(retirement.is_finished());
}
