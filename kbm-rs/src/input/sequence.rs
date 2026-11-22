//! Additional one-call key taps. No firmware changes or private wire opcodes.
use super::*;

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SequenceResult {
    pub usb_completed: bool,
    pub completed_keys: usize,
    pub packets: usize,
}

fn key(name: &str) -> Result<Vec<u16>, AppError> {
    if name.len() > 32 {
        return Err(invalid());
    }
    if name.len() == 1 {
        let c = name.as_bytes()[0];
        let shifted = c.is_ascii_uppercase();
        let usage = match c {
            b'a'..=b'z' => u16::from(c - b'a' + 4),
            b'A'..=b'Z' => u16::from(c - b'A' + 4),
            b'1'..=b'9' => u16::from(c - b'1' + 30),
            b'0' => 39,
            b' ' => 44,
            b'\n' | b'\r' => 40,
            b'\t' => 43,
            b'-' => 45,
            b'=' => 46,
            b'[' => 47,
            b']' => 48,
            b'\\' => 49,
            b';' => 51,
            b'\'' => 52,
            b'`' => 53,
            b',' => 54,
            b'.' => 55,
            b'/' => 56,
            _ => 0,
        };
        if usage != 0 {
            return Ok(if shifted {
                vec![225, usage]
            } else {
                vec![usage]
            });
        }
        if let Some(at) = "!@#$%^&*()".bytes().position(|s| s == c) {
            return Ok(vec![225, if at == 9 { 39 } else { 30 + at as u16 }]);
        }
        if let Some(at) = "_+{}|:\"~<>?".bytes().position(|s| s == c) {
            return Ok(vec![225, [45, 46, 47, 48, 49, 51, 52, 53, 54, 55, 56][at]]);
        }
    }
    let usage = match name.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(224),
        "shift" => Some(225),
        "alt" => Some(226),
        "win" | "meta" | "super" => Some(227),
        "esc" => Some(41),
        _ => crate::keyboard_layout::chord_keys()
            .iter()
            .find(|k| k.label.eq_ignore_ascii_case(name))
            .map(|k| k.usage),
    }
    .ok_or_else(invalid)?;
    Ok(vec![usage])
}
fn invalid() -> AppError {
    AppError::new("INVALID_KEY_SEQUENCE", "키 배열은 1~128개의 지원 키 이름/ASCII 문자여야 합니다. Unicode 입력이나 키 조합 문자열은 지원하지 않습니다.")
}
fn plan(keys: &[String]) -> Result<Vec<Vec<(u16, u16)>>, AppError> {
    if keys.is_empty() || keys.len() > 128 {
        return Err(invalid());
    }
    // Never split a DOWN/UP pair (including Shift) across a bounded burst.
    // Eight commands fit both legacy and current target keyboard FIFO limits.
    let mut bursts: Vec<Vec<(u16, u16)>> = vec![Vec::new()];
    for name in keys {
        let usages = key(name)?;
        let burst = bursts.last_mut().unwrap();
        if burst.len() + usages.len() * 2 > 8 {
            bursts.push(Vec::new());
        }
        let burst = bursts.last_mut().unwrap();
        burst.extend(usages.iter().map(|&u| (0x101, u)));
        burst.extend(usages.iter().rev().map(|&u| (0x102, u)));
    }
    Ok(bursts)
}

impl InputManager {
    pub async fn key_sequence(
        self: &Arc<Self>,
        owner: Owner,
        binding: Binding,
        keys: Vec<String>,
    ) -> Result<SequenceResult, AppError> {
        let bursts = plan(&keys)?; // validate every item before the first packet
        self.prepare_owner(&owner, &binding)?;
        let original = binding.clone();
        let manager = self.clone();
        let permit = self.reserve()?;
        let count = keys.len();
        let result = tokio::spawn(async move {
            let _permit = permit;
            let _gate = manager.keyboard_gate.lock().await;
            manager.claim(&owner, &binding)?;
            if manager.state.lock().held.as_ref().is_some_and(|h| !h.keys.is_empty()) {
                return Err(AppError::new("INPUT_BUSY", "누르고 있는 키를 해제한 뒤 키 배열을 전송하세요."));
            }
            let mut packets = 0;
            for burst in bursts {
                if let Err(error) = manager.claim(&owner, &binding) {
                    if packets == 0 { return Err(error); }
                    return Err(AppError::uncertain("HID_SEQUENCE_FAILED",
                        format!("키 배열 일부가 이미 실행됐습니다({}). 전체를 재시도하지 마세요.",error.code)).at(&binding));
                }
                let possible: BTreeSet<_> = burst.iter().map(|p| p.1).collect();
                if let Some(h) = manager.state.lock().held.as_mut() { h.keys.extend(&possible); }
                // Poll in wire order, pre-queue paired UPs without waiting for
                // DOWN's HID_DONE. All are ordinary requests: cleanup priority
                // would otherwise allow an UP to overtake its DOWN.
                let pending = burst.iter().map(|&(op,usage)| {
                    manager.send_pinned(binding.clone(), owner.clone(), op,
                                        usage.to_le_bytes().to_vec(), false)
                });
                let results = futures_util::future::join_all(pending).await;
                packets += burst.len();
                if let Some(error) = results.into_iter().find_map(Result::err) {
                    // Finish all submitted futures, stop later bursts, then
                    // attempt only UP/neutral on the original session. Never
                    // retry DOWN or carry the batch onto a reconnected MCU.
                    for usage in &possible {
                        let _ = manager.send_pinned(binding.clone(), owner.clone(), 0x102,
                            usage.to_le_bytes().to_vec(), true).await;
                    }
                    let _ = manager.recover_source_locked(binding.clone(), owner.clone()).await;
                    return Err(AppError::uncertain("HID_SEQUENCE_FAILED",
                        format!("키 배열이 일부 실행되었을 수 있습니다({}). 전체를 자동 재시도하지 마세요.", error.code)).at(&binding));
                }
                if let Some(h) = manager.state.lock().held.as_mut()
                    .filter(|h| h.binding == binding && h.owner.id == owner.id) {
                    for usage in &possible { h.keys.remove(usage); }
                }
            }
            Ok(SequenceResult { usb_completed:true, completed_keys:count, packets })
        }).await.map_err(|_| AppError::uncertain("INPUT_FAILED", "키 배열 처리 작업이 종료됐습니다."))?
          .map_err(|e| e.at(&original));
        if let Some(logger) = self
            .logger
            .lock()
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
        {
            logger.command("hid_key_sequence", Some(&original), &result);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    type Calls = Arc<Mutex<Vec<(u16, u16, bool, u64)>>>;
    #[derive(Default)]
    struct Wire {
        calls: Calls,
        pause_first: AtomicBool,
        fail_first: AtomicBool,
        epoch: Arc<AtomicU64>,
        submitted: Arc<tokio::sync::Notify>,
        resume: Arc<tokio::sync::Notify>,
    }
    impl Backend for Wire {
        fn epoch(&self, _: &Binding) -> Result<Epoch, AppError> {
            Ok(Epoch {
                connection: self.epoch.load(Ordering::Acquire),
                session: 1,
            })
        }
        fn ready(&self, _: &Binding) -> Result<(), AppError> {
            Ok(())
        }
        fn send(
            &self,
            b: Binding,
            owner: Owner,
            op: u16,
            payload: Vec<u8>,
            cleanup: bool,
        ) -> BoxFuture<'static, Result<u32, AppError>> {
            let calls = self.calls.clone();
            let epoch = self.epoch.clone();
            let submitted = self.submitted.clone();
            let resume = self.resume.clone();
            let pause = self.pause_first.swap(false, Ordering::AcqRel);
            let fail = self.fail_first.swap(false, Ordering::AcqRel);
            Box::pin(async move {
                let identity = epoch.load(Ordering::Acquire);
                owner.validate_pin(
                    &b,
                    Epoch {
                        connection: identity,
                        session: 1,
                    },
                )?;
                let usage = payload
                    .get(..2)
                    .map(|p| u16::from_le_bytes(p.try_into().unwrap()))
                    .unwrap_or(0);
                calls.lock().push((op, usage, cleanup, identity));
                submitted.notify_one();
                if pause {
                    resume.notified().await;
                }
                owner.validate_pin(
                    &b,
                    Epoch {
                        connection: epoch.load(Ordering::Acquire),
                        session: 1,
                    },
                )?;
                if fail {
                    return Err(AppError::uncertain("HID_UNCONFIRMED", "test"));
                }
                Ok(1)
            })
        }
    }
    fn setup() -> (Arc<InputManager>, Arc<Wire>, Binding, Owner) {
        let active = Arc::new(ActiveCoordinator::default());
        active.begin().unwrap();
        active.finish(Some(Uid::from_bytes(&[1; 12]).unwrap()));
        let b = active.bind().unwrap();
        let wire = Arc::new(Wire::default());
        let manager = InputManager::new(active, wire.clone());
        let owner = Owner {
            pin: Default::default(),
            id: "sequence".into(),
            cancelled: Default::default(),
            command_guard: None,
        };
        (manager, wire, b, owner)
    }
    async fn submitted(w: &Wire, n: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while w.calls.lock().len() < n {
                w.submitted.notified().await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn abc_pairs_are_queued_before_first_down_completes() {
        let (m, w, b, o) = setup();
        w.pause_first.store(true, Ordering::Release);
        let task = tokio::spawn({
            let m = m.clone();
            let b = b.clone();
            async move {
                m.key_sequence(o, b, vec!["a".into(), "b".into(), "c".into()])
                    .await
            }
        });
        submitted(&w, 6).await;
        assert!(!task.is_finished());
        assert_eq!(
            w.calls
                .lock()
                .iter()
                .map(|p| (p.0, p.1, p.2))
                .collect::<Vec<_>>(),
            vec![
                (0x101, 4, false),
                (0x102, 4, false),
                (0x101, 5, false),
                (0x102, 5, false),
                (0x101, 6, false),
                (0x102, 6, false)
            ]
        );
        w.resume.notify_one();
        let result = task.await.unwrap().unwrap();
        assert_eq!((result.completed_keys, result.packets), (3, 6));
        assert_eq!(m.snapshot(&b).keys_held, 0);
    }
    #[tokio::test]
    async fn invalid_tail_and_held_keyboard_never_start_a_sequence() {
        let (m, w, b, o) = setup();
        assert_eq!(
            m.key_sequence(o.clone(), b.clone(), vec!["a".into(), "한".into()])
                .await
                .err()
                .unwrap()
                .code,
            "INVALID_KEY_SEQUENCE"
        );
        assert!(w.calls.lock().is_empty());
        m.apply(
            o.clone(),
            b.clone(),
            Action::Key {
                down: true,
                usage: 4,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            m.key_sequence(o, b, vec!["b".into()])
                .await
                .err()
                .unwrap()
                .code,
            "INPUT_BUSY"
        );
        assert_eq!(w.calls.lock().len(), 1);
    }
    #[tokio::test]
    async fn partial_failure_stops_future_bursts_and_only_releases_original_keys() {
        let (m, w, b, o) = setup();
        w.fail_first.store(true, Ordering::Release);
        let error = m
            .key_sequence(o, b.clone(), vec!["a".into(); 9])
            .await
            .err()
            .unwrap();
        assert!(error.uncertain);
        let calls = w.calls.lock();
        assert_eq!(calls.iter().filter(|p| !p.2).count(), 8);
        assert_eq!(calls.iter().filter(|p| p.0 == 0x101).count(), 4);
        assert!(calls
            .iter()
            .filter(|p| p.2)
            .all(|p| matches!(p.0, 0x102 | 0x107)));
        assert_eq!(m.snapshot(&b).keys_held, 0);
    }
    #[tokio::test]
    async fn reconnect_never_sends_cleanup_or_later_burst_to_new_connection() {
        let (m, w, b, o) = setup();
        w.pause_first.store(true, Ordering::Release);
        let task = tokio::spawn({
            let m = m.clone();
            async move { m.key_sequence(o, b, vec!["a".into(); 9]).await }
        });
        submitted(&w, 8).await;
        w.epoch.store(2, Ordering::Release);
        w.resume.notify_one();
        assert!(task.await.unwrap().is_err());
        assert!(w.calls.lock().iter().all(|p| p.3 == 0));
    }
    #[test]
    fn shifted_keys_remain_whole_and_limits_are_validated() {
        assert_eq!(
            plan(&["A".into(), "!".into()]).unwrap(),
            vec![vec![
                (0x101, 225),
                (0x101, 4),
                (0x102, 4),
                (0x102, 225),
                (0x101, 225),
                (0x101, 30),
                (0x102, 30),
                (0x102, 225)
            ]]
        );
        assert!(plan(&[]).is_err());
        assert!(plan(&vec!["a".into(); 129]).is_err());
        assert!(plan(&["Ctrl+a".into()]).is_err());
        for key in crate::keyboard_layout::chord_keys() {
            assert!(plan(&[key.label]).is_ok());
        }
    }
    #[tokio::test]
    async fn cancellation_stops_after_the_already_queued_pairs() {
        let (m, w, b, o) = setup();
        w.pause_first.store(true, Ordering::Release);
        let task = tokio::spawn({
            let m = m.clone();
            let b = b.clone();
            let o = o.clone();
            async move { m.key_sequence(o, b, vec!["a".into(); 9]).await }
        });
        submitted(&w, 8).await;
        o.cancelled.cancel();
        w.resume.notify_one();
        let error = task.await.unwrap().err().unwrap();
        assert_eq!(error.code, "HID_SEQUENCE_FAILED");
        assert!(error.uncertain);
        assert_eq!(w.calls.lock().len(), 8);
        assert_eq!(m.snapshot(&b).keys_held, 0);
    }
}
