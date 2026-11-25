use super::args::*;
use serde_json::json;

fn parse(values: &[&str]) -> std::io::Result<Options> {
    Options::parse(values.iter().map(|s| s.to_string()))
}
#[test]
fn paths_and_framing_stay_on_local_transport() {
    for path in [
        "http://example.com/api",
        "//example.com/api",
        "ipc://server/api",
        "/foo#fragment",
        "/foo\r\nHost:x",
    ] {
        assert!(local_path(path).is_err(), "{path}");
    }
    assert_eq!(
        local_path("ipc:/api/v1/state?a=1").unwrap(),
        "/api/v1/state?a=1"
    );
    for header in [
        "Host:evil",
        "Authorization:token",
        "Content-Length:1",
        "Transfer-Encoding:chunked",
    ] {
        assert!(parse(&["request", "POST", "/", "--header", header]).is_err());
    }
    assert!(parse(&["state", "--json", "{}", "--data-binary", "a.bin"]).is_err());
    assert!(parse(&["state", "--profile", "a", "--pipe", "b"]).is_err());
}
#[test]
fn every_service_command_has_a_cli_request_plan() {
    let mut names = std::collections::HashSet::new();
    for doc in crate::service::COMMANDS {
        assert!(names.insert(doc.name));
        let command = Command {
            method: doc.method.into(),
            path: doc.path.into(),
        };
        let plan = command_plan(&command, json!({})).unwrap();
        assert_eq!(plan.method, doc.method);
        assert_eq!(plan.path, doc.path);
    }
}
#[test]
fn json_arrays_and_utf8_queries_keep_their_meaning() {
    let command = Command {
        method: "POST".into(),
        path: "/api/v1/test".into(),
    };
    let sequence = json!(["a", "b", "c"]);
    assert_eq!(
        command_plan(&command, sequence.clone()).unwrap().json,
        Some(sequence)
    );
    let command = Command {
        method: "GET".into(),
        ..command
    };
    let query = command_plan(&command, json!({"q":"한 &+","limit":2})).unwrap();
    assert!(query.path.contains("q=%ED%95%9C%20%26%2B"));
    assert!(command_plan(&command, json!({"q":[1]})).is_err());
    let options = parse(&["call", "key_sequence", "--json", "[\"a\",\"b\"]"]).unwrap();
    assert_eq!(options.arguments().unwrap(), json!(["a", "b"]));
}

#[test]
fn conventional_fields_follow_live_schema_and_preserve_string_values() {
    let options = parse(&[
        "firmware_upload_prepare",
        "--target",
        "ms2130",
        "--name",
        "0123",
        "--size",
        "32",
    ])
    .unwrap();
    let schema = crate::remote::input_schema("firmware_upload_prepare");
    assert_eq!(
        options.apply_fields(json!({}), Some(&schema)).unwrap(),
        json!({"target":"ms2130","name":"0123","size":32})
    );
    assert!(!options.wants_json());
    let options = parse(&["hid_key_down", "--usage", "0x04", "--input-session", "123"]).unwrap();
    assert_eq!(
        options
            .apply_fields(
                json!({}),
                Some(&crate::remote::input_schema("hid_key_down"))
            )
            .unwrap(),
        json!({"usage":4,"input_session":"123"})
    );
    let options = parse(&["hid_key_sequence", "--keys", "a", "--keys=b", "--keys", "c"]).unwrap();
    assert_eq!(
        options
            .apply_fields(
                json!({}),
                Some(&crate::remote::input_schema("hid_key_sequence"))
            )
            .unwrap(),
        json!({"keys":["a","b","c"]})
    );
}
#[test]
fn nested_fields_booleans_and_invalid_options_do_not_change_request_meaning() {
    let options = parse(&[
        "call",
        "network_forward_start",
        "--rules.0.id",
        "test",
        "--rules.0.protocol",
        "tcp",
        "--rules.0.listen-port",
        "18880",
        "--rules.0.target-ip",
        "192.168.127.100",
        "--rules.0.target-port",
        "80",
    ])
    .unwrap();
    let value = options
        .apply_fields(
            json!({}),
            Some(&crate::remote::input_schema("network_forward_start")),
        )
        .unwrap();
    assert_eq!(value["rules"][0]["listen_port"], 18880);
    assert_eq!(value["rules"][0]["target_ip"], "192.168.127.100");
    let options = parse(&[
        "storage_upload_prepare",
        "--kind",
        "optical",
        "--name",
        "boot.iso",
        "--size",
        "81920",
        "--confirmed",
    ])
    .unwrap();
    assert_eq!(
        options
            .apply_fields(
                json!({}),
                Some(&crate::remote::input_schema("storage_upload_prepare"))
            )
            .unwrap()["confirmed"],
        true
    );
    for values in [
        vec!["--usgae", "4"],
        vec!["--usage"],
        vec!["--usage", "4", "--usage", "5"],
    ] {
        let options = parse(&values).unwrap();
        assert!(options
            .apply_fields(
                json!({}),
                Some(&crate::remote::input_schema("hid_key_down"))
            )
            .is_err());
    }
    assert!(parse(&["hid_key_down", "--usage", "4", "--json", "{}"]).is_err());
    assert!(parse(&["events", "--raw", "--pretty"]).is_err());
}
#[test]
fn json_output_switch_and_legacy_json_body_are_both_supported() {
    assert!(parse(&["--json", "channels"]).unwrap().wants_json());
    let options = parse(&["hid_key_down", "--json", "{\"usage\":4}"]).unwrap();
    assert!(options.wants_json());
    assert_eq!(options.arguments().unwrap(), json!({"usage":4}));
    assert!(parse(&["hid_key_down", "--usage", "4", "--json"])
        .unwrap()
        .wants_json());
}
#[test]
fn human_output_is_readable_and_event_decoder_handles_partial_utf8_and_sse_frames() {
    let table = super::human::render(
        &json!({"commands":[{"name":"app_get_state","method":"GET","title":"앱 상태"}]}),
    );
    assert!(table.contains("NAME"));
    assert!(table.contains("app_get_state"));
    assert!(!table.contains('{'));
    let channels = super::human::render(
        &json!({"channels":[{"uid":"0x01","name":"채널","connected":true,"target_state":1}],"error":"","revision":1}),
    );
    assert!(channels.contains("CONNECTED"));
    assert!(channels.contains("채널"));
    assert!(!channels.contains("channels:"));
    assert!(
        super::human::render(&json!({"channels":[],"error":"discovery failed"}))
            .contains("Discovery error: discovery failed")
    );
    assert!(
        super::human::error(&json!({"code":"NO_ACTIVE_CHANNEL","message":"No channel"}))
            .starts_with("Error NO_ACTIVE_CHANNEL:")
    );
    let safe = super::human::render(&json!({"name":"\u{1b}[2J"}));
    assert!(!safe.contains('\u{1b}'));
    let bytes="id: 1\r\nevent: state\r\ndata: {\"name\":\"채널\"}\r\n\r\n: keepalive\n\nid: 2\ndata: false\n\n".as_bytes();
    let mut decoder = super::human::Events::default();
    let mut events = Vec::new();
    for chunk in bytes.chunks(1) {
        events.extend(decoder.push(chunk).unwrap());
    }
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["data"]["name"], "채널");
    assert_eq!(events[1]["data"], false);
}
