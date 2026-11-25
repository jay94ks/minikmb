//! Shared response contracts and offline API reference. No commands run here.
use crate::{
    model::{AppError, Uid},
    service::CommandDoc,
};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize, JsonSchema)]
pub struct Bound<T> {
    pub uid: Uid,
    pub generation: u64,
    #[serde(flatten)]
    pub value: T,
}
macro_rules! response {
    ($name:ident {$($field:ident : $ty:ty),* $(,)?}) => {
        #[derive(Serialize,JsonSchema)]
        pub struct $name {$(pub $field:$ty),*}
    };
}
response!(AppState {
    state: crate::service::Snapshot,
    servers: crate::remote::RemoteStatus
});
response!(ActiveChannel {
    channel: crate::model::Channel,
    generation: u64
});
response!(VideoClosed { running: bool });
response!(Closed { closed: bool });
response!(Receiving { receiving: bool });
response!(UsbCompleted {
    usb_completed: bool
});
response!(FolderOpened {
    requested: bool,
    confirmed: bool
});
response!(WakeDetails {
    config: crate::preferences::Wol,
    usb_wake_supported: bool,
    system_control_supported: bool
});
response!(WakeConfig {
    config: crate::preferences::Wol
});
response!(WakeAdapters {adapters:Vec<crate::wake::Adapter>});
response!(FirmwareImages {images:Vec<crate::firmware::sources::Image>});
response!(FirmwareProgrammers {programmers:Vec<crate::firmware::backend::Programmer>});
response!(FirmwareStatus {job:Option<crate::firmware::manager::Info>});
response!(WakeSent {
    code: u16,
    message: String,
    wake_confirmed: bool
});
response!(StorageDetails {
    snapshot:Option<crate::storage_wire::Snapshot>,job:Option<crate::storage::JobInfo>,
    resources:Option<crate::storage::media::ResourceInfo>,cleanup_error:Option<AppError>,
    card_format:Option<crate::storage::card_format::Info>,folder_sources:Vec<crate::storage::folder::SourceInfo>
});
response!(UploadPrepared {
    upload_id: String,
    url: String,
    method: String,
    content_type: String,
    content_length: u64,
    expires_at: u64,
    single_use: bool,
    instructions: String,
    example: String
});
response!(ChannelDiagnostic {connected:bool,features:u32,firmware:[u16;3],slot:Option<u8>,capture_paired:bool,target_state:u8});
response!(InputDiagnostic {
    ready: bool,
    keys_held: usize,
    buttons_held: usize,
    release_unconfirmed: bool,
    error_present: bool
});
response!(TtyDiagnostic {
    receiving: bool,
    bytes_received: u64,
    bytes_retained: usize,
    bytes_overwritten: u64,
    bytes_accepted: u64,
    tx_discarded: u32,
    rx_discarded: u32,
    error_present: bool
});
response!(NetworkDiagnostic {mode:crate::network::Mode,mcu:crate::network::framing::McuStats,cleanup_required:bool,error_code:Option<String>});
response!(JobDiagnostic {kind:String,state:crate::storage::JobState,done:u64,total:u64,can_cancel:bool,error_code:Option<String>,cleanup_error_code:Option<String>});
response!(LoggingDiagnostic {dropped:u64,write_error:Option<String>});
response!(DiagnosticDetails {
    observed_at:u64,channel:Option<ChannelDiagnostic>,mcu_fresh:bool,transport:Option<crate::diagnostics::Transport>,
    input:InputDiagnostic,video:crate::video::VideoStatus,tty:Option<TtyDiagnostic>,
    network:NetworkDiagnostic,storage_job:Option<JobDiagnostic>,logging:LoggingDiagnostic
});
fn schema<T: JsonSchema>() -> Value {
    serde_json::to_value(
        schemars::generate::SchemaSettings::default()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<T>(),
    )
    .unwrap()
}
pub fn response_schema(name: &str) -> Option<Value> {
    Some(match name {
        "firmware_images" => schema::<FirmwareImages>(),
        "firmware_programmers" => schema::<FirmwareProgrammers>(),
        "firmware_inspect" => schema::<crate::firmware::backend::Inspection>(),
        "firmware_upload_prepare" => schema::<crate::firmware::sources::Upload>(),
        "firmware_start" | "firmware_job_get" | "firmware_job_cancel" => {
            schema::<crate::firmware::manager::Info>()
        }
        "firmware_get_status" => schema::<FirmwareStatus>(),
        "events_get_snapshot" => schema::<crate::events::Report>(),
        "app_get_state" => schema::<AppState>(),
        "channels_list" => schema::<crate::model::Inventory>(),
        "channel_get_active" => schema::<ActiveChannel>(),
        "channel_activate" | "channel_deactivate" => schema::<crate::active::ActiveState>(),
        "video_open" | "video_get_status" => schema::<crate::video::VideoStatus>(),
        "video_close" => schema::<VideoClosed>(),
        "video_snapshot" => schema::<crate::video::FrameInfo>(),
        "input_open" | "input_heartbeat" => schema::<crate::input_sessions::SessionInfo>(),
        "input_close" => schema::<Bound<Closed>>(),
        "hid_get_status" => schema::<Bound<crate::input::InputStatus>>(),
        "hid_recover" => schema::<Bound<UsbCompleted>>(),
        "hid_key_down" | "hid_key_up" | "hid_button_down" | "hid_button_up" | "hid_mouse_move"
        | "hid_wheel" | "hid_chord" | "hid_click" | "hid_double_click" | "hid_release" => {
            schema::<Bound<crate::input::InputResult>>()
        }
        "hid_key_sequence" => schema::<Bound<crate::input::SequenceResult>>(),
        "tty_open" | "tty_get_status" => schema::<crate::tty::Status>(),
        "tty_close" => schema::<Bound<Receiving>>(),
        "tty_read" => schema::<crate::tty::ReadResult>(),
        "tty_write" => schema::<crate::tty::WriteResult>(),
        "storage_get_status" => schema::<Bound<StorageDetails>>(),
        "storage_upload_prepare" => schema::<UploadPrepared>(),
        "storage_action" | "storage_job_get" | "storage_job_cancel" | "storage_upload_bytes" => {
            schema::<crate::storage::JobInfo>()
        }
        "network_get_status" | "network_configure" | "network_set_mode" => {
            schema::<crate::network::manager::Status>()
        }
        "network_forward_configure" | "network_forward_start" | "network_forward_stop" => {
            schema::<crate::network::forwarding::Status>()
        }
        "network_scan_start" | "network_scan_stop" => schema::<crate::network::scanner::Status>(),
        "wake_get_status" => schema::<Bound<WakeDetails>>(),
        "wake_configure" => schema::<WakeConfig>(),
        "wake_list_adapters" => schema::<WakeAdapters>(),
        "wake_send" => schema::<Bound<WakeSent>>(),
        "logs_get_state" | "logs_configure" | "logs_flush" => schema::<crate::logging::Status>(),
        "logs_open_folder" => schema::<FolderOpened>(),
        "diagnostics_get" => schema::<Bound<DiagnosticDetails>>(),
        _ => return None,
    })
}
pub fn error_schema() -> Value {
    schema::<crate::remote_error::Failure>()
}
pub fn group(name: &str) -> &'static str {
    if name.starts_with("channel") || name == "app_get_state" || name == "events_get_snapshot" {
        "채널·앱"
    } else if name.starts_with("video_") {
        "영상"
    } else if name.starts_with("hid_") || name.starts_with("input_") {
        "키보드·마우스"
    } else if name.starts_with("tty_") {
        "시리얼"
    } else if name.starts_with("storage_") {
        "디스크·SD 카드"
    } else if name.starts_with("firmware_") {
        "펌웨어"
    } else if name.starts_with("network_") {
        "네트워크"
    } else if name.starts_with("wake_") {
        "PC 깨우기"
    } else {
        "로그·진단"
    }
}
pub const GROUPS: &[&str] = &[
    "채널·앱",
    "영상",
    "키보드·마우스",
    "시리얼",
    "디스크·SD 카드",
    "네트워크",
    "펌웨어",
    "PC 깨우기",
    "로그·진단",
];
pub fn matches(command: &CommandDoc, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty()
        || [
            command.name,
            command.method,
            command.path,
            command.title,
            command.description,
            group(command.name),
        ]
        .iter()
        .any(|s| s.to_lowercase().contains(&q))
}
pub fn example_args(name: &str) -> Value {
    match name {
        "firmware_programmers" => json!({"target":"stm32"}),
        "firmware_inspect" => json!({"target":"stm32","programmer_id":"0483:3748"}),
        "firmware_upload_prepare" => json!({"target":"ms2130","name":"firmware.bin","size":64468}),
        "firmware_start" => {
            json!({"target":"stm32","programmer_id":"0483:3748","image_id":"minikmb-fs-hid-r63","confirmed":true})
        }
        "firmware_job_get" | "firmware_job_cancel" => json!({"job_id":"<JOB_ID>"}),
        "channel_activate" => json!({"uid":"0x010101010101010101010101"}),
        "video_open" => json!({}),
        "video_snapshot" => json!({"format":"jpeg"}),
        "input_heartbeat" | "input_close" => json!({"input_session":"<INPUT_SESSION>"}),
        "hid_key_down" | "hid_key_up" => json!({"usage":4}),
        "hid_button_down" | "hid_button_up" => json!({"button":1}),
        "hid_mouse_move" => json!({"x":0,"y":0}),
        "hid_click" => json!({"x":0,"y":0,"button":1,"count":1,"hold_ms":30}),
        "hid_double_click" => json!({"x":0,"y":0,"button":1,"hold_ms":30}),
        "hid_key_sequence" => json!({"keys":["a","b","c"]}),
        "hid_wheel" => json!({"vertical":1,"horizontal":0}),
        "hid_chord" => json!({"usages":[224,4],"hold_ms":30}),
        "tty_write" => json!({"bytes":[72,101,108,108,111,13,10]}),
        "tty_read" => json!({"max_bytes":4096}),
        "storage_upload_prepare" => {
            json!({"kind":"optical","name":"boot.iso","size":81920,"confirmed":true})
        }
        "storage_action" => json!({"action":"refresh"}),
        "storage_job_get" | "storage_job_cancel" => json!({"job_id":"<JOB_ID>"}),
        "network_configure" => serde_json::to_value(crate::network::Config::default()).unwrap(),
        "network_set_mode" => json!({"mode":"off"}),
        "network_forward_configure" | "network_forward_start" => {
            json!({"rules":[{"id":"example","protocol":"tcp","listen_port":18080,"target_ip":"192.168.127.100","target_port":80}]})
        }
        "network_scan_start" => {
            json!({"target":"192.168.127.100","start":80,"end":80,"timeout_ms":1000})
        }
        "wake_configure" => json!({"enabled":false,"mac":"","local_address":""}),
        "logs_configure" => json!({"enabled":true,"verbose":false}),
        _ => json!({}),
    }
}

// Standalone schemars documents use #/$defs references. OpenAPI embeds many
// documents, so lift definitions into separate, operation-scoped components.
fn lift(mut schema: Value, prefix: &str, components: &mut serde_json::Map<String, Value>) -> Value {
    let defs = schema
        .as_object_mut()
        .and_then(|s| s.remove("$defs"))
        .and_then(|d| d.as_object().cloned())
        .unwrap_or_default();
    let names: std::collections::HashMap<_, _> = defs
        .keys()
        .enumerate()
        .map(|(i, k)| (k.clone(), format!("{prefix}_{i}")))
        .collect();
    fn rewrite(value: &mut Value, names: &std::collections::HashMap<String, String>) {
        match value {
            Value::Object(o) => {
                if let Some(reference) = o.get_mut("$ref") {
                    if let Some(key) = reference.as_str().and_then(|s| s.strip_prefix("#/$defs/")) {
                        let key = key.replace("~1", "/").replace("~0", "~");
                        if let Some(name) = names.get(&key) {
                            *reference = Value::String(format!("#/components/schemas/{name}"));
                        }
                    }
                }
                for value in o.values_mut() {
                    rewrite(value, names);
                }
            }
            Value::Array(a) => {
                for value in a {
                    rewrite(value, names);
                }
            }
            _ => {}
        }
    }
    rewrite(&mut schema, &names);
    for (name, mut definition) in defs {
        rewrite(&mut definition, &names);
        components.insert(names[&name].clone(), definition);
    }
    schema
}
pub fn openapi(address: Option<std::net::SocketAddr>) -> Value {
    let mut paths = serde_json::Map::new();
    let mut schemas = serde_json::Map::new();
    let error = lift(error_schema(), "error", &mut schemas);
    for c in crate::service::COMMANDS {
        let request = lift(
            crate::remote::input_schema(c.name),
            &format!("{}_in", c.name),
            &mut schemas,
        );
        let response = lift(
            response_schema(c.name).expect("every command needs a response contract"),
            &format!("{}_out", c.name),
            &mut schemas,
        );
        let mut responses = json!({"200":{"description":"성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다.","content":{"application/json":{"schema":response}}}});
        for (status, description) in ERROR_STATUSES {
            responses[*status] =
                json!({"description":description,"content":{"application/json":{"schema":error}}});
        }
        let mut op = json!({"operationId":c.name,"tags":[group(c.name)],"summary":c.title,"description":c.description,"security":[{"bearer":[]}],"responses":responses,"x-active-required":c.active_required,"x-input-schema":request,"x-mcp-output-schema":response});
        if matches!(c.method, "POST" | "PUT") {
            op["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":request,"example":example_args(c.name)}}});
        }
        if c.name == "hid_key_sequence" {
            op["requestBody"]["content"]["application/json"]["schema"] = json!({"oneOf":[request,{"type":"array","minItems":1,"maxItems":128,"items":{"type":"string"}}]});
        }
        if c.name == "video_snapshot" {
            op["parameters"] = json!([{"name":"format","in":"query","schema":{"type":"string","enum":["jpeg","png"],"default":"jpeg"}}]);
            op["responses"]["200"] = snapshot_response();
        }
        let entry = paths.entry(c.path.to_owned()).or_insert(json!({}));
        entry[c.method.to_ascii_lowercase()] = op;
    }
    let job = lift(
        response_schema("storage_upload_bytes").unwrap(),
        "upload_bytes_out",
        &mut schemas,
    );
    paths.insert("/api/v1/active/storage/uploads/{upload_id}".into(),json!({"post":{
        "operationId":"storage_upload_bytes","tags":["디스크·SD 카드"],"summary":"준비한 이미지 파일 스트리밍",
        "description":"storage_upload_prepare가 반환한 url에 같은 API 키로 파일 원문을 한 번 POST합니다. Content-Length는 content_length와 같아야 합니다. chunked/query/inline MCP 본문은 지원하지 않습니다. 수신 완료 뒤 job_id로 SD 검증 완료를 조회하세요.",
        "security":[{"bearer":[]}],"parameters":[{"name":"upload_id","in":"path","required":true,"schema":{"type":"string"}},{"name":"Content-Length","in":"header","required":true,"schema":{"type":"integer","minimum":1}}],
        "requestBody":{"required":true,"content":{"application/octet-stream":{"schema":{"type":"string","format":"binary"}}}},
        "responses":{"200":{"description":"앱의 SD 작업 수락. 완료는 job_id로 조회합니다.","content":{"application/json":{"schema":job}}},"default":{"description":"인증·단회 주소·크기/형식·활성 연결 또는 한도 오류","content":{"application/json":{"schema":error}}}}
    }}));
    let firmware_source = lift(
        schema::<crate::firmware::sources::Source>(),
        "firmware_upload_out",
        &mut schemas,
    );
    paths.insert("/api/v1/firmware/uploads/{upload_id}".into(),json!({"post":{
        "operationId":"firmware_upload_bytes","tags":["펌웨어"],"summary":"준비한 펌웨어 업로드",
        "description":"firmware_upload_prepare의 단회 URL에 같은 Bearer 키로 파일 바이트를 POST합니다. Content-Length 일치, query/chunked 없음. 응답 source_id로 firmware_start를 별도 호출해야 기록됩니다.",
        "security":[{"bearer":[]}],"parameters":[{"name":"upload_id","in":"path","required":true,"schema":{"type":"string"}},{"name":"Content-Length","in":"header","required":true,"schema":{"type":"integer","minimum":1,"maximum":67108864}}],
        "requestBody":{"required":true,"content":{"application/octet-stream":{"schema":{"type":"string","format":"binary"}}}},
        "responses":{"200":{"description":"수신/이미지 검증 완료. 아직 기록하지 않았습니다.","content":{"application/json":{"schema":firmware_source}}},"default":{"description":"인증·주소·크기·형식 오류","content":{"application/json":{"schema":error}}}}
    }}));
    let event_schema = lift(
        schema::<crate::events::Envelope>(),
        "event_out",
        &mut schemas,
    );
    paths.insert("/api/v1/events".into(),json!({"get":{"operationId":"events_subscribe","summary":"캐시 상태 변경 구독","description":"Bearer 인증 SSE. 첫 이벤트는 최신 전체 상태이며 이후 최대 4Hz로 병합합니다. 재접속/Last-Event-ID는 이력을 재생하지 않습니다. state/resync를 처리하고 resync에서는 events_get_snapshot을 재조회합니다. 15초 keepalive; 앱 전체32/키별8 구독. 키 폐기·서버 OFF·종료 시 닫힙니다.","security":[{"bearer":[]}],"responses":{"200":{"description":"SSE id=epoch:sequence, event=state/resync, data=Envelope JSON. 영상/TTY/파일 본문과 작업 result 제외.","content":{"text/event-stream":{"schema":{"type":"string"}}},"x-event-data-schema":event_schema}}}}));
    paths.insert("/api/v1/openapi.json".into(),json!({"get":{"operationId":"openapi_get","summary":"현재 API 계약","security":[{"bearer":[]}],"responses":{"200":{"description":"OpenAPI 3.1 JSON 문서","content":{"application/json":{"schema":{"type":"object"}}}}}}}));
    paths.insert("/api/v1/reference".into(),json!({"get":{"operationId":"api_reference_page","summary":"REST API 테스트 페이지","description":"같은 REST 서버에서 제공하는 자체 포함 HTML입니다. 페이지 열기에는 키가 필요 없지만 실제 API 요청에는 기존 Bearer 인증이 필요합니다. Host/Origin 제한을 유지하고 키를 저장하지 않습니다.","security":[],"responses":{"200":{"description":"API 설명·인자·허용값·요청 편집·응답 보기","content":{"text/html":{"schema":{"type":"string"}}}}}}}));
    for path in paths.values_mut() {
        if let Some(operations) = path.as_object_mut() {
            for operation in operations.values_mut() {
                if let Some(responses) = operation
                    .get_mut("responses")
                    .and_then(Value::as_object_mut)
                {
                    for (code, description) in ERROR_STATUSES {
                        responses.entry(*code).or_insert_with(||json!({"description":description,"content":{"application/json":{"schema":error}}}));
                    }
                    for (code, response) in responses.iter_mut() {
                        if response.get("headers").is_none() {
                            response["headers"] = json!({});
                        }
                        response["headers"]["x-minikmb-request-id"] = json!({"description":"서버 생성 요청 ID. 오류 본문의 request_id와 같습니다.","schema":{"type":"string"}});
                        response["headers"]["cache-control"] =
                            json!({"schema":{"type":"string","const":"no-store"}});
                        if code == "401" {
                            response["headers"]["www-authenticate"] = json!({"schema":{"type":"string","const":"Bearer realm=\"MiniKMB\""}});
                        }
                        if code == "405" {
                            response["headers"]["allow"] = json!({"schema":{"type":"string"}});
                        }
                    }
                }
            }
        }
    }
    let servers=address.map_or_else(||json!([{"url":"/","description":"서버 OFF에서는 상대 주소입니다. 시작 후 표시되는 실제 주소를 사용하세요."}]),|a|json!([{"url":format!("http://{a}")} ]));
    json!({"openapi":"3.1.0","info":{"title":"MiniKMB API","version":env!("CARGO_PKG_VERSION"),"description":OVERVIEW},"servers":servers,"tags":GROUPS.iter().map(|name|json!({"name":name})).collect::<Vec<_>>(),"paths":paths,"components":{"schemas":schemas,"securitySchemes":{"bearer":{"type":"http","scheme":"bearer"}}}})
}
pub const ERROR_STATUSES: &[(&str, &str)] = &[
    (
        "400",
        "인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요.",
    ),
    (
        "401",
        "API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다.",
    ),
    (
        "403",
        "다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다.",
    ),
    ("404", "채널·작업·단회 업로드 주소를 찾을 수 없습니다."),
    (
        "405",
        "지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요.",
    ),
    ("413", "허용한 요청 본문 크기를 넘었습니다."),
    ("415", "Content-Type이 올바르지 않습니다."),
    (
        "409",
        "활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다.",
    ),
    (
        "429",
        "요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요.",
    ),
    ("501", "현재 앱/펌웨어가 지원하지 않습니다."),
    ("503", "필요한 서버가 꺼져 있거나 종료 중입니다."),
];
pub const OVERVIEW:&str="UI·REST·MCP 전체의 활성 채널은 하나입니다. channel_activate만 UID를 받으며 일반 제어는 현재 활성 채널을 고정합니다. 입력 좌표는 캡처 이미지 픽셀입니다. 문서·예제 복사·리소스 읽기는 제어 요청을 실행하지 않습니다. API 키 원문은 예제에 넣지 않습니다. 성공 응답과 장치/타깃의 실제 결과를 구분하고 uncertain 오류의 DOWN/쓰기를 재시도하지 마세요.";
fn snapshot_response() -> Value {
    let mut headers = serde_json::Map::new();
    for (name, description) in [
        ("x-minikmb-uid", "0x+24hex 채널 UID"),
        ("x-minikmb-generation", "활성 세대"),
        ("x-minikmb-frame", "프레임 참조"),
        ("x-minikmb-stream", "영상 스트림 참조"),
        ("x-minikmb-width", "캡처 너비(픽셀)"),
        ("x-minikmb-height", "캡처 높이(픽셀)"),
        ("x-minikmb-received-at", "프레임 수신 시각(Unix ms)"),
    ] {
        headers.insert(
            name.into(),
            json!({"description":description,"schema":{"type":"string"}}),
        );
    }
    headers.insert(
        "cache-control".into(),
        json!({"schema":{"type":"string","const":"no-store"}}),
    );
    json!({"description":"최근1초 이내의 실제 캡처 이미지. MCP는 image content와 FrameInfo structuredContent를 함께 반환합니다.","headers":headers,"content":{"image/jpeg":{"schema":{"type":"string","format":"binary"}},"image/png":{"schema":{"type":"string","format":"binary"}}}})
}

pub struct GuideTopic {
    pub title: &'static str,
    pub body: &'static str,
    pub commands: &'static [&'static str],
}
pub const GUIDE_TOPICS:&[GuideTopic]=&[
    GuideTopic{title:"상태 이벤트",body:"REST GET /api/v1/events는 Bearer 인증 SSE입니다. 연결 직후 전체 최신 state를 받고 이후 변경은 약250ms 단위로 병합합니다. id=epoch:sequence의 누락은 중간 상태가 병합됐다는 뜻입니다. 재접속 시 이력을 재생하지 않으며 Last-Event-ID는 복원 토큰이 아닙니다. payload_limit resync를 받으면 events_get_snapshot 또는 GET /api/v1/events/snapshot으로 전체를 읽습니다. MCP는 minikmb://events/snapshot 등 고정 상태 URI의 변경 알림 뒤 resources/read로 재조회합니다. 2025-11-25는 resources/subscribe·unsubscribe, 2026-07-28은 subscriptions/listen·요청 취소를 사용합니다. 전체32/키별8 구독; URI 알림과 상태만 전달하며 키 입력·영상·TTY/파일 본문·job.result는 보내지 않습니다. 완료 결과는 storage_job_get으로 확인하세요. 구독은 HID/영상/SD/NCM 기능을 시작하지 않습니다.",commands:&["events_get_snapshot"]},
    GuideTopic{title:"연결·키·서버",body:"외부 제어에서 키를 만들면 OS 난수 키를 생성하고 클립보드에 복사합니다. 여러 키를 만들고 개별 삭제할 수 있습니다. 키 원문은 다시 조회하지 않습니다. MCP ON은 REST도 켜며 MCP OFF는 REST를 유지하고 REST OFF는 둘 다 끕니다. 현재 서버는 이 PC의 127.0.0.1에만 바인딩합니다. 다른 PC에서 이 loopback 주소로 연결할 수 있다고 가정하지 마세요. Streamable HTTP와 매 요청 Authorization: Bearer <API_KEY>를 사용합니다. 세션 ID가 인증을 대체하지 않습니다. 키 삭제는 기존 MCP 세션·입력·진행 중 요청을 종료합니다.",commands:&["app_get_state"]},
    GuideTopic{title:"채널·영상·마우스",body:"channels_list에서 실제 UID와 고정 허브 묶음을 읽고 channel_activate로 하나만 선택합니다. GUI/다른 키/MCP/REST가 활성 채널을 공유합니다. 일반 명령에 UID/세대/복수 채널 목록을 넣지 마세요. video_open 뒤 video_get_status에서 첫 프레임을 확인하고 video_snapshot으로 JPEG/PNG를 받습니다. 마우스 x/y는 캡처 이미지의 0..width-1 / 0..height-1 픽셀이며 내부 HID 좌표가 아닙니다. frame_id를 보내면 오래된 영상/크기 참조를 거부합니다. HID 완료는 타깃 앱이 동작했다는 증거가 아닙니다.",commands:&["channels_list","channel_activate","channel_get_active","video_open","video_get_status","video_snapshot","hid_mouse_move","hid_click"]},
    GuideTopic{title:"키보드·드래그·해제",body:"단발 입력은 기본 lease로 사용할 수 있습니다. 여러 호출 사이에 키/버튼을 유지하려면 input_open의 input_session을 DOWN/이동/UP에 넣고 만료30초 전에 heartbeat를 보냅니다. 키보드는 Unicode 문자열 대신 USB HID usage입니다. 다른 소유자가 누르고 있으면 INPUT_BUSY이며 기존 입력을 섞지 않습니다. 정상 종료는 같은 세션의 UP/release/close를 사용합니다. DOWN의 uncertain 오류는 다시 보내지 마세요. 원래 관리 연결에서만 자동 해제하며 미확인 상태는 새 입력을 막습니다. 명시적 hid_recover는 현재 연결 전체를 해제하므로 복구 의도가 있을 때만 호출합니다. 가상 키 조합과 물리 Ctrl+Alt+Delete의 호스트 보안 동작은 구분합니다.",commands:&["input_open","input_heartbeat","hid_key_down","hid_key_up","hid_button_down","hid_button_up","hid_wheel","hid_chord","hid_release","input_close","hid_get_status","hid_recover"]},
    GuideTopic{title:"시리얼 중계",body:"tty_open으로 현재 타깃 CDC의 수신을 시작합니다. tty_read에 마지막 응답 cursor와 stream_id를 전달해 이어 읽고 skipped로 덮어쓰기를 확인합니다. tty_write는 bytes 배열이며 accepted/total/complete/error를 함께 확인합니다. 부분 수락·불확실 송신을 통째로 재시도하면 데이터가 중복될 수 있습니다. tty_close는 추가 credit을 중지하며 이미 허용된 데이터는 폐기될 수 있습니다. 관리 USB COM의 baud를 바꾸는 기능이 아닙니다.",commands:&["tty_open","tty_get_status","tty_read","tty_write","tty_close"]},
    GuideTopic{title:"펌웨어 기록",body:"채널 선택 없이 firmware_images와 firmware_programmers를 조회하고 연결한 ST-Link/Writer를 선택합니다. firmware_inspect는 선택적 용량 조회이며 관리 USB나 MCU UID 대조를 요구하지 않습니다. firmware_start는 image_id 또는 source_id 하나와 confirmed=true를 받습니다. 다른 파일은 firmware_upload_prepare가 반환하는 REST URL에 같은 Bearer 키로 POST한 뒤 source_id를 사용하세요. 파일 내용/호스트 경로를 MCP에 넣지 않습니다. 플래시가 이미지보다 작으면 지우기 전에 오류입니다. 작업이 활성 채널 자원을 해제하므로 완료 조회는 원래 job_id를 사용합니다. 기록 시작 후 취소/종료를 기다리고 state=completed와 verified를 확인하세요. MS2130 ROM은 다음 보드 재기동부터 적용됩니다.",commands:&["firmware_images","firmware_programmers","firmware_inspect","firmware_upload_prepare","firmware_start","firmware_job_get","firmware_job_cancel","firmware_get_status"]},
    GuideTopic{title:"큰 파일·SD 작업",body:"MCP에는 ISO/UDF나 디스크 파일 본문을 넣지 마세요. storage_upload_prepare에 kind/name 또는 format/size/confirmed를 보내 반환된 url·content_type·content_length를 사용합니다. 에이전트 자신의 파일을 같은 API 키로 REST POST하며 Content-Length는 준비한 크기와 같아야 합니다. chunked/query는 지원하지 않습니다. 주소는10분 동안 유효한 단회 주소이고 진행 중 idle 한도는30초입니다. POST 응답의 job_id로 storage_job_get을 반복해 state=completed 및 error/cleanup_error/result를 확인합니다. HTTP 수신이나 작업 수락은 SD 검증 완료가 아닙니다. 이미 사용된 주소의 POST를 무조건 재시도하지 마세요. 취소는 storage_job_cancel 후 상태를 재조회합니다. 수락된 전체 카드 포맷은 취소할 수 없습니다.",commands:&["storage_get_status","storage_upload_prepare","storage_job_get","storage_job_cancel"]},
    GuideTopic{title:"디스크·광학·카드·폴더",body:"storage_action의 action별 스키마를 REST 문서에서 확인합니다. refresh/card_info/name_get/name_set/user_read/user_write, optical_set/optical_mount, disk_create/disk_format/disk_open/disk_close/disk_sync/disk_read/disk_write, card_format/card_format_status, folder_share/folder_forget을 제공합니다. 이미지 형식은1=FAT32,2=exFAT,3=EXT4이며 전체 카드 포맷은1/2만 지원합니다. disk_create는64..32768MiB입니다. 변경은 confirmed와 현재 카드 UUID·이미지 용량·lease 같은 expected 값을 사용해 대상을 확인합니다. direct I/O 쓰기는512바이트 섹터입니다. 폴더는 앱에서 직접 선택한 source_id만 사용하며 임의 호스트 경로를 받지 않습니다. BOARD.ID는 내부 파일로 목록/공유에 노출하지 않습니다. 작업 결과의 result는 action과 phase에 따라 달라지는 JSON 값입니다.",commands:&["storage_action"]},
    GuideTopic{title:"NCM·공유·포워딩·스캐너",body:"네트워크는 시작 OFF이며 설정을 읽거나 저장해도 자동으로 켜지지 않습니다. network_set_mode의 emulation은 타깃 NCM을, host는 이 PC의 현재 경로를 사용하는 TCP/UDP·DNS 공유를 켭니다. NIC/route/방화벽/ICS 설정을 임의 변경하지 않습니다. 새 loopback 포워딩은 rules를 저장한 뒤 명시적으로 시작합니다. 타깃은 해당 NCM /24 안의 한 주소이며 스캐너도 같은 타깃 범위에서만 실행합니다. 요청/창 종료와 별개로 수락된 공유·포워딩 자원은 앱 소유로 유지될 수 있으므로 명시적으로 stop/off합니다. cleanup_required이면 원래 채널에서 off로 복구한 뒤 진행합니다.",commands:&["network_get_status","network_configure","network_set_mode","network_forward_configure","network_forward_start","network_forward_stop","network_scan_start","network_scan_stop"]},
    GuideTopic{title:"PC 깨우기·로그·오류 해결",body:"WOL은 실제 PC LAN 어댑터와 저장한 MAC으로 명시적으로 한 번 발신합니다. code128은 발신/기상 미확인,129는 비활성,130은 미지원입니다. 현재 펌웨어의 USB wake/System Control 미지원은 성공으로 대체하지 않습니다. diagnostics_get은 캐시 신선도와 카운터만 읽으며 기능을 켜지 않습니다. 로그는 유한 메타데이터이며 키 입력·TTY/파일/패킷 본문·API 키를 저장하지 않습니다. 401은 키/세션, NO_ACTIVE_CHANNEL은 선택, STALE_SESSION/STALE_OPERATION은 원래 연결 수명, HID_RELEASE_UNCONFIRMED는 원래 MCU의 해제 미확인 기록이며 후속 입력이나 채널 전환을 영구 차단하지 않습니다. request_id는 HTTP x-minikmb-request-id와 같고, channel_uid/generation은 고정했던 원래 채널을 가리킵니다. 실제 대상을 고정하기 전 오류는 접수 시 관찰한 활성 채널 또는 null입니다. MCU_REJECTED의 status와 uncertain도 함께 확인합니다. 연결 점검에서 HID·포맷·네트워크 ON을 자동 실행하지 마세요.",commands:&["wake_get_status","wake_list_adapters","wake_configure","wake_send","diagnostics_get","logs_get_state","logs_configure","logs_flush","logs_open_folder"]},
    GuideTopic{title:"한 호출로 키 배열·더블클릭",body:"기존 DOWN/UP·hid_chord·hid_click/count는 유지됩니다. hid_double_click은 영상 픽셀 x/y에서 두 번 클릭합니다. hid_key_sequence의 keys:[\"a\",\"b\",\"c\"]는 a DOWN/UP→b DOWN/UP→c DOWN/UP으로 처리하며 다음 LLM 호출이나 CoT를 기다리지 않습니다. REST POST /api/v1/active/hid/keys/sequence는 배열 본문도 받습니다. 1~128개 지원 키 이름/ASCII, US 물리 키 매핑이며 Unicode 문자열 입력은 아닙니다. Enter/Tab/F1~F24/Lang1도 지원합니다. 전체 사전 검증 후 한 활성 채널/관리 세션에 고정하고 DOWN/UP을 같은 작은 묶음으로 미리 큐에 넣습니다. HID_SEQUENCE_FAILED/uncertain 뒤 전체를 재시도하지 마세요. 펌웨어 변경 없이 호스트 부하에 대한 실제 시간 간격까지 보장하지는 않습니다.",commands:&["hid_click","hid_double_click","hid_key_sequence"]},
];
pub fn mcp_config(address: Option<std::net::SocketAddr>) -> Value {
    let a = address.unwrap_or_else(|| "127.0.0.1:18181".parse().unwrap());
    json!({"transport":"streamable-http","url":format!("http://{a}/mcp"),"headers":{"Authorization":"Bearer <API_KEY>"}})
}
pub fn mcp_guide(rest: Option<std::net::SocketAddr>, mcp: Option<std::net::SocketAddr>) -> String {
    let mut out=format!("# MiniKMB MCP·REST 연동\n\n{OVERVIEW}\n\nREST: {}\n\nMCP: {}\n\n```json\n{}\n```\n\n이 예시는 클라이언트별 설정 파일 형식을 강제하지 않습니다. 서버OFF의 주소는 연결 예시입니다.\n",rest.map_or("OFF".into(),|a|format!("http://{a}")),mcp.map_or("OFF".into(),|a|format!("http://{a}/mcp")),serde_json::to_string_pretty(&mcp_config(mcp)).unwrap());
    for topic in GUIDE_TOPICS {
        out.push_str(&format!(
            "\n## {}\n\n{}\n\n관련 도구: {}\n",
            topic.title,
            topic.body,
            topic.commands.join(", ")
        ));
    }
    out.push_str("\n## 읽기 전용 리소스\n\n리소스 URI는 고정이며 UID/경로/query를 받지 않습니다. 상태 URI는 변경 알림 뒤 다시 읽습니다. 정적 목록 변경 알림은 광고하지 않습니다. 현재 상태는 캐시 TTL0/private입니다.\n\n");
    for r in crate::remote_resources::RESOURCES {
        out.push_str(&format!("- `{}` — {}: {}\n", r.uri, r.name, r.description));
    }
    out.push_str("\n## 전체 도구\n\n");
    for c in crate::service::COMMANDS {
        out.push_str(&format!(
            "- `{}` — {} ({})\n",
            c.name,
            c.title,
            group(c.name)
        ));
    }
    out
}

#[cfg(test)]
mod tests;
