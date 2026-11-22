//! Typed GUI-independent control service. REST and MCP use the same dispatch.
use crate::{
    active::{Binding, PixelPoint},
    input::{Action, Owner},
    input_sessions::Principal,
    model::AppError,
    service::AppService,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionArgs {
    pub input_session: Option<String>,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{input::Backend, model::Uid};
    use parking_lot::Mutex;
    use tokio_util::sync::CancellationToken;
    type Calls = Vec<(Uid, u16, Vec<u8>, bool)>;
    #[derive(Default)]
    pub(crate) struct Wire {
        pub calls: Mutex<Calls>,
        pub fail_release: std::sync::atomic::AtomicBool,
        pub pause_mouse: std::sync::atomic::AtomicBool,
        pub fail_mouse: std::sync::atomic::AtomicBool,
        pub mouse_entered: Arc<tokio::sync::Notify>,
        pub mouse_resume: Arc<tokio::sync::Notify>,
    }
    impl Backend for Wire {
        fn epoch(&self, _: &Binding) -> Result<crate::input::Epoch, AppError> {
            Ok(Default::default())
        }
        fn ready(&self, _: &Binding) -> Result<(), AppError> {
            Ok(())
        }
        fn send(
            &self,
            b: Binding,
            _: Owner,
            op: u16,
            payload: Vec<u8>,
            cleanup: bool,
        ) -> futures_util::future::BoxFuture<'static, Result<u32, AppError>> {
            self.calls.lock().push((b.uid, op, payload, cleanup));
            let fail = (op == 0x107
                && self.fail_release.load(std::sync::atomic::Ordering::Acquire))
                || (op == 0x103 && self.fail_mouse.load(std::sync::atomic::Ordering::Acquire));
            let pause = op == 0x103
                && self
                    .pause_mouse
                    .swap(false, std::sync::atomic::Ordering::AcqRel);
            let entered = self.mouse_entered.clone();
            let resume = self.mouse_resume.clone();
            Box::pin(async move {
                if pause {
                    entered.notify_one();
                    resume.notified().await;
                }
                if fail {
                    Err(AppError::uncertain(
                        "HID_UNCONFIRMED",
                        "test release timeout",
                    ))
                } else {
                    Ok(1)
                }
            })
        }
    }
    pub(crate) fn setup() -> (tempfile::TempDir, Arc<AppService>, Arc<Wire>) {
        let dir = tempfile::tempdir().unwrap();
        let wire = Arc::new(Wire::default());
        let app = AppService::with_test_input(dir.path().into(), wire.clone()).unwrap();
        let channels = (1..=2)
            .map(|n| crate::model::Channel {
                uid: Uid::from_bytes(&[n; 12]).unwrap(),
                name: format!("test-{n}"),
                port: String::new(),
                connected: true,
                features: 1,
                target_state: 1,
                firmware: [1, 0, 0],
                board_id: "test".into(),
                usb_port: u32::from(n),
                slot: Some(n - 1),
                capture_id: None,
                pairing_error: String::new(),
                observed_at: 0,
            })
            .collect();
        app.inventory.write().channels = channels;
        app.active.begin().unwrap();
        app.active.finish(Some(Uid::from_bytes(&[1; 12]).unwrap()));
        app.test_video_geometry(1920, 1080);
        (dir, app, wire)
    }
    fn principal(id: &str) -> Principal {
        Principal {
            key_id: id.into(),
            client_id: "rest".into(),
            cancelled: CancellationToken::new(),
            ended: CancellationToken::new(),
        }
    }
    #[tokio::test]
    async fn remote_owner_pixel_geometry_and_channel_switch_use_one_service() {
        let (_dir, app, wire) = setup();
        let a = principal("A");
        let b = principal("B");
        let opened = app
            .execute_remote(&a, "input_open", json!({}))
            .await
            .unwrap();
        let id = opened["input_session"].as_str().unwrap();
        let key = app
            .execute_remote(&a, "hid_key_down", json!({"usage":4,"input_session":id}))
            .await
            .unwrap();
        assert_eq!(key["usb_completed"], true);
        assert_eq!(key["uid"], opened["uid"]);
        assert_eq!(
            app.execute_remote(&b, "hid_key_down", json!({"usage":5,"input_session":id}))
                .await
                .err()
                .unwrap()
                .code,
            "FORBIDDEN"
        );
        assert_eq!(
            app.execute_remote(&b, "hid_key_down", json!({"usage":5}))
                .await
                .err()
                .unwrap()
                .code,
            "INPUT_BUSY"
        );
        let frame = app.current_frame().unwrap().info;
        app.execute_remote(
            &a,
            "hid_mouse_move",
            json!({"x":1919,"y":1079,"frame_id":frame.frame_id,"input_session":id}),
        )
        .await
        .unwrap();
        assert_eq!(
            wire.calls.lock().last().unwrap().2,
            vec![255, 127, 255, 127]
        );
        let count = wire.calls.lock().len();
        assert_eq!(
            app.execute_remote(
                &a,
                "hid_mouse_move",
                json!({"x":1920,"y":0,"input_session":id})
            )
            .await
            .err()
            .unwrap()
            .code,
            "INVALID_COORDINATES"
        );
        assert_eq!(
            app.execute_remote(
                &a,
                "hid_mouse_move",
                json!({"x":0,"y":0,"frame_id":"1-999-1","input_session":id})
            )
            .await
            .err()
            .unwrap()
            .code,
            "STALE_FRAME_REFERENCE"
        );
        assert_eq!(wire.calls.lock().len(), count);
        app.select(Some(Uid::from_bytes(&[2; 12]).unwrap()))
            .await
            .unwrap();
        assert_eq!(wire.calls.lock().last().unwrap().1, 0x107);
        let count = wire.calls.lock().len();
        assert_eq!(
            app.execute_remote(&a, "hid_key_up", json!({"usage":4,"input_session":id}))
                .await
                .err()
                .unwrap()
                .code,
            "STALE_OPERATION"
        );
        assert_eq!(wire.calls.lock().len(), count);
    }
    #[tokio::test]
    async fn closing_and_key_revocation_release_only_the_lease_owner() {
        let (_dir, app, wire) = setup();
        let a = principal("A");
        let b = principal("B");
        let opened = app
            .execute_remote(&a, "input_open", json!({}))
            .await
            .unwrap();
        let id = opened["input_session"].as_str().unwrap();
        app.execute_remote(&a, "hid_key_down", json!({"usage":4,"input_session":id}))
            .await
            .unwrap();
        assert_eq!(
            app.execute_remote(&b, "input_close", json!({"input_session":id}))
                .await
                .err()
                .unwrap()
                .code,
            "FORBIDDEN"
        );
        assert_eq!(wire.calls.lock().len(), 1);
        app.execute_remote(&a, "input_close", json!({"input_session":id}))
            .await
            .unwrap();
        assert_eq!(wire.calls.lock().last().unwrap().1, 0x107);
        app.execute_remote(&b, "hid_key_down", json!({"usage":5}))
            .await
            .unwrap();
        b.cancelled.cancel();
        for id in app.input_sessions.reap(&app.active) {
            app.input.release_owner(id).await.unwrap();
        }
        assert_eq!(wire.calls.lock().last().unwrap().1, 0x107);
        assert_eq!(app.input.snapshot(&app.active.bind().unwrap()).keys_held, 0);
    }
    #[tokio::test]
    async fn routing_fields_and_changed_stream_guard_are_rejected_before_wire() {
        let (_dir, app, wire) = setup();
        let p = principal("A");
        for args in [
            json!({"usage":4,"uid":"0101"}),
            json!({"usage":4,"uids":[]}),
        ] {
            assert_eq!(
                app.execute_remote(&p, "hid_key_down", args)
                    .await
                    .err()
                    .unwrap()
                    .code,
                "INVALID_ARGUMENTS"
            );
        }
        let b = app.active.bind().unwrap();
        let (action, guard) = app
            .pixel_action(&b, PixelPoint { x: 0.0, y: 0.0 }, None)
            .unwrap();
        guard.cancel();
        let owner = Owner {
            pin: Default::default(),
            id: "test-queued-geometry".into(),
            cancelled: CancellationToken::new(),
            command_guard: Some(guard),
        };
        assert_eq!(
            app.input.apply(owner, b, action).await.err().unwrap().code,
            "STALE_FRAME_REFERENCE"
        );
        assert!(wire.calls.lock().is_empty());
    }
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionIdArgs {
    pub input_session: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyArgs {
    pub usage: u16,
    pub input_session: Option<String>,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ButtonArgs {
    pub button: u8,
    pub input_session: Option<String>,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveArgs {
    pub x: f64,
    pub y: f64,
    pub frame_id: Option<String>,
    pub input_session: Option<String>,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WheelArgs {
    pub vertical: i16,
    #[serde(default)]
    pub horizontal: i16,
    pub input_session: Option<String>,
}
fn hold() -> u64 {
    30
}
fn one() -> u8 {
    1
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChordArgs {
    pub usages: Vec<u16>,
    #[serde(default = "hold")]
    pub hold_ms: u64,
    pub input_session: Option<String>,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClickArgs {
    pub x: f64,
    pub y: f64,
    #[serde(default = "one")]
    pub button: u8,
    #[serde(default = "one")]
    pub count: u8,
    #[serde(default = "hold")]
    pub hold_ms: u64,
    pub frame_id: Option<String>,
    pub input_session: Option<String>,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DoubleClickArgs {
    pub x: f64,
    pub y: f64,
    #[serde(default = "one")]
    pub button: u8,
    #[serde(default = "hold")]
    pub hold_ms: u64,
    pub frame_id: Option<String>,
    pub input_session: Option<String>,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SequenceArgs {
    #[schemars(length(min = 1, max = 128))]
    pub keys: Vec<String>,
    pub input_session: Option<String>,
}
fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, AppError> {
    serde_json::from_value(v).map_err(|_| {
        AppError::new(
            "INVALID_ARGUMENTS",
            "인자 형식을 확인하세요. 일반 입력에는 UID를 지정하지 않습니다.",
        )
    })
}
fn completed(binding: &Binding, result: crate::input::InputResult) -> Value {
    let mut v = json!(result);
    v["uid"] = json!(binding.uid);
    v["generation"] = json!(binding.generation);
    v
}
async fn apply(
    app: &Arc<AppService>,
    owner: Owner,
    b: Binding,
    action: Action,
) -> Result<Value, AppError> {
    let result = app.input.apply(owner, b.clone(), action).await?;
    Ok(completed(&b, result))
}
pub fn is_command(name: &str) -> bool {
    name.starts_with("hid_") || name.starts_with("input_")
}
pub fn schema(name: &str) -> Option<Value> {
    Some(match name {
        "input_open" | "hid_get_status" | "hid_recover" => {
            serde_json::to_value(schemars::schema_for!(Empty)).unwrap()
        }
        "input_heartbeat" | "input_close" => {
            serde_json::to_value(schemars::schema_for!(SessionIdArgs)).unwrap()
        }
        "hid_key_down" | "hid_key_up" => {
            serde_json::to_value(schemars::schema_for!(KeyArgs)).unwrap()
        }
        "hid_button_down" | "hid_button_up" => {
            serde_json::to_value(schemars::schema_for!(ButtonArgs)).unwrap()
        }
        "hid_mouse_move" => serde_json::to_value(schemars::schema_for!(MoveArgs)).unwrap(),
        "hid_wheel" => serde_json::to_value(schemars::schema_for!(WheelArgs)).unwrap(),
        "hid_chord" => serde_json::to_value(schemars::schema_for!(ChordArgs)).unwrap(),
        "hid_click" => serde_json::to_value(schemars::schema_for!(ClickArgs)).unwrap(),
        "hid_double_click" => serde_json::to_value(schemars::schema_for!(DoubleClickArgs)).unwrap(),
        "hid_key_sequence" => serde_json::to_value(schemars::schema_for!(SequenceArgs)).unwrap(),
        "hid_release" => serde_json::to_value(schemars::schema_for!(SessionArgs)).unwrap(),
        _ => return None,
    })
}
impl AppService {
    pub fn pixel_action(
        &self,
        binding: &Binding,
        point: PixelPoint,
        reference: Option<&str>,
    ) -> Result<(Action, tokio_util::sync::CancellationToken), AppError> {
        (|| {
            self.active.validate(binding)?;
            let frame = self.current_frame()?;
            if frame.info.uid != binding.uid || frame.info.generation != binding.generation {
                return Err(AppError::new("STALE_OPERATION", "채널이 변경됐습니다."));
            }
            if let Some(id) = reference {
                self.validate_frame_reference(binding, id)?;
            }
            let [x, y] = point.hid(frame.info.width, frame.info.height)?;
            Ok((Action::Move { x, y }, frame.life))
        })()
        .map_err(|e: AppError| e.at(binding))
    }
    pub async fn execute_remote(
        self: &Arc<Self>,
        principal: &Principal,
        name: &str,
        args: Value,
    ) -> Result<Value, AppError> {
        let binding = self.active.bind().ok();
        let result = self.execute_remote_inner(principal, name, args).await;
        if !crate::network_api::is_command(name)
            && !crate::firmware_api::is_command(name)
            && !crate::storage_api::is_command(name)
            && !crate::tty_api::is_command(name)
            && !crate::wake::is_command(name)
            && !crate::observability::is_command(name)
            && !name.starts_with("hid_")
            && !["channel_activate", "channel_deactivate"].contains(&name)
        {
            return self.audit(name, binding.as_ref(), result);
        }
        result
    }
    async fn execute_remote_inner(
        self: &Arc<Self>,
        principal: &Principal,
        name: &str,
        args: Value,
    ) -> Result<Value, AppError> {
        if crate::firmware_api::is_command(name) {
            return self.execute_firmware(None, principal, name, args).await;
        }
        if crate::observability::is_command(name) {
            return self.execute_observability(principal, name, args).await;
        }
        if crate::wake::is_command(name) {
            return self
                .execute_wake(self.active.bind()?, principal, name, args)
                .await;
        }
        if crate::network_api::is_command(name) {
            return self
                .execute_network(self.active.bind()?, principal, name, args)
                .await;
        }
        if crate::storage_api::is_command(name) {
            return self.execute_storage(None, principal, name, args);
        }
        if crate::tty_api::is_command(name) {
            return self
                .execute_tty(self.active.bind()?, principal, name, args)
                .await;
        }
        if !is_command(name) {
            return self.execute(name, args).await;
        }
        if principal.cancelled.is_cancelled() || principal.ended.is_cancelled() {
            return Err(AppError::new(
                "UNAUTHORIZED",
                "인증 또는 연결이 종료됐습니다.",
            ));
        }
        match name {
            "input_open" => {
                let _: Empty = parse(args)?;
                let info = self.input_sessions.open(&self.active, principal)?;
                let prepared = self
                    .input_sessions
                    .resolve(&self.active, principal, Some(&info.input_session))
                    .and_then(|(owner, binding)| self.input.prepare_owner(&owner, &binding));
                if let Err(error) = prepared {
                    let _ = self.input_sessions.close(principal, &info.input_session);
                    return Err(error);
                }
                Ok(json!(info))
            }
            "input_heartbeat" => {
                let a: SessionIdArgs = parse(args)?;
                let (owner, binding) =
                    self.input_sessions
                        .resolve(&self.active, principal, Some(&a.input_session))?;
                self.input.prepare_owner(&owner, &binding)?;
                Ok(json!(self.input_sessions.heartbeat(
                    &self.active,
                    principal,
                    &a.input_session
                )?))
            }
            "input_close" => {
                let a: SessionIdArgs = parse(args)?;
                let (id, b) = self.input_sessions.close(principal, &a.input_session)?;
                self.input.release_owner(id).await.map_err(|e| e.at(&b))?;
                Ok(json!({"closed":true,"uid":b.uid,"generation":b.generation}))
            }
            "hid_get_status" => {
                let _: Empty = parse(args)?;
                let b = self.active.bind()?;
                let state = self.input.snapshot(&b);
                self.active.validate(&b)?;
                let mut state = json!(state);
                state["uid"] = json!(b.uid);
                state["generation"] = json!(b.generation);
                Ok(state)
            }
            "hid_recover" => {
                let _: Empty = parse(args)?;
                let b = self.active.bind()?;
                self.input.recover(b.clone()).await.map_err(|e| e.at(&b))?;
                Ok(json!({"usb_completed":true,"uid":b.uid,"generation":b.generation}))
            }
            "hid_key_down" | "hid_key_up" => {
                let a: KeyArgs = parse(args)?;
                let action = Action::Key {
                    down: name == "hid_key_down",
                    usage: a.usage,
                };
                action.packet()?;
                let (owner, b) = self.input_sessions.resolve(
                    &self.active,
                    principal,
                    a.input_session.as_deref(),
                )?;
                apply(self, owner, b, action).await
            }
            "hid_button_down" | "hid_button_up" => {
                let a: ButtonArgs = parse(args)?;
                let action = Action::Button {
                    down: name == "hid_button_down",
                    button: a.button,
                };
                action.packet()?;
                let (owner, b) = self.input_sessions.resolve(
                    &self.active,
                    principal,
                    a.input_session.as_deref(),
                )?;
                apply(self, owner, b, action).await
            }
            "hid_mouse_move" => {
                let a: MoveArgs = parse(args)?;
                let (mut owner, b) = self.input_sessions.resolve(
                    &self.active,
                    principal,
                    a.input_session.as_deref(),
                )?;
                let (action, guard) =
                    self.pixel_action(&b, PixelPoint { x: a.x, y: a.y }, a.frame_id.as_deref())?;
                owner.command_guard = Some(guard);
                apply(self, owner, b, action).await
            }
            "hid_wheel" => {
                let a: WheelArgs = parse(args)?;
                let action = Action::Wheel {
                    vertical: a.vertical,
                    horizontal: a.horizontal,
                };
                action.packet()?;
                let (owner, b) = self.input_sessions.resolve(
                    &self.active,
                    principal,
                    a.input_session.as_deref(),
                )?;
                apply(self, owner, b, action).await
            }
            "hid_chord" => {
                let a: ChordArgs = parse(args)?;
                let (owner, b) = self.input_sessions.resolve(
                    &self.active,
                    principal,
                    a.input_session.as_deref(),
                )?;
                let result = self
                    .input
                    .chord(owner, b.clone(), a.usages, a.hold_ms)
                    .await?;
                Ok(completed(&b, result))
            }
            "hid_key_sequence" => {
                let args = if args.is_array() {
                    json!({"keys": args})
                } else {
                    args
                };
                let a: SequenceArgs = parse(args)?;
                let (owner, b) = self.input_sessions.resolve(
                    &self.active,
                    principal,
                    a.input_session.as_deref(),
                )?;
                let result = self.input.key_sequence(owner, b.clone(), a.keys).await?;
                let mut value = json!(result);
                value["uid"] = json!(b.uid);
                value["generation"] = json!(b.generation);
                Ok(value)
            }
            "hid_click" | "hid_double_click" => {
                let a: ClickArgs = if name == "hid_double_click" {
                    let d: DoubleClickArgs = parse(args)?;
                    ClickArgs {
                        x: d.x,
                        y: d.y,
                        button: d.button,
                        count: 2,
                        hold_ms: d.hold_ms,
                        frame_id: d.frame_id,
                        input_session: d.input_session,
                    }
                } else {
                    parse(args)?
                };
                let (mut owner, b) = self.input_sessions.resolve(
                    &self.active,
                    principal,
                    a.input_session.as_deref(),
                )?;
                let (action, guard) =
                    self.pixel_action(&b, PixelPoint { x: a.x, y: a.y }, a.frame_id.as_deref())?;
                owner.command_guard = Some(guard);
                let Action::Move { x, y } = action else {
                    unreachable!()
                };
                let result = self
                    .input
                    .click(
                        owner,
                        b.clone(),
                        crate::input::Click {
                            x,
                            y,
                            button: a.button,
                            count: a.count,
                            hold_ms: a.hold_ms,
                        },
                    )
                    .await?;
                Ok(completed(&b, result))
            }
            "hid_release" => {
                let a: SessionArgs = parse(args)?;
                let (owner, b) = self.input_sessions.resolve(
                    &self.active,
                    principal,
                    a.input_session.as_deref(),
                )?;
                apply(self, owner, b, Action::Release).await
            }
            _ => Err(AppError::new(
                "NOT_IMPLEMENTED",
                "지원하지 않는 입력 명령입니다.",
            )),
        }
    }
    pub async fn ui_move(
        self: &Arc<Self>,
        mut owner: Owner,
        binding: Binding,
        point: PixelPoint,
    ) -> Result<crate::input::InputResult, AppError> {
        let (action, guard) = self.pixel_action(&binding, point, None)?;
        owner.command_guard = Some(guard);
        self.input.apply(owner, binding, action).await
    }
}
