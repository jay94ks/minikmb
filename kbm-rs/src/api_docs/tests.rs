use super::*;
fn valid(schema: &Value, value: &Value) {
    let v = jsonschema::validator_for(schema).unwrap();
    if let Err(error) = v.validate(value) {
        panic!("response/example failed schema: {error}");
    }
}
#[test]
fn every_command_has_a_valid_request_example_and_typed_response() {
    for c in crate::service::COMMANDS {
        let input = crate::remote::input_schema(c.name);
        let validator = jsonschema::validator_for(&input).unwrap();
        let example = example_args(c.name);
        assert!(
            validator.is_valid(&example),
            "{}: {}",
            c.name,
            validator
                .iter_errors(&example)
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        );
        let output = response_schema(c.name).unwrap_or_else(|| panic!("{} lacks output", c.name));
        assert_eq!(output["type"], "object", "{}", c.name);
        jsonschema::validator_for(&output).unwrap();
    }
    assert!(response_schema("unknown").is_none());
}
#[test]
fn embedded_openapi_references_resolve_and_binary_contract_is_explicit() {
    let doc = openapi(Some("127.0.0.1:21981".parse().unwrap()));
    assert_eq!(doc["servers"][0]["url"], "http://127.0.0.1:21981");
    fn refs(v: &Value, root: &Value) {
        match v {
            Value::Object(o) => {
                if let Some(r) = o.get("$ref").and_then(Value::as_str) {
                    assert!(r.starts_with("#/components/schemas/"), "unlifted {r}");
                    assert!(root.pointer(&r[1..]).is_some(), "unresolved {r}");
                }
                for value in o.values() {
                    refs(value, root);
                }
            }
            Value::Array(a) => {
                for value in a {
                    refs(value, root);
                }
            }
            _ => {}
        }
    }
    refs(&doc, &doc);
    let snapshot = &doc["paths"]["/api/v1/active/video/snapshot"]["get"];
    assert!(snapshot["responses"]["200"]["content"]["image/png"].is_object());
    assert!(snapshot["responses"]["200"]["headers"]["x-minikmb-frame"].is_object());
    let upload = &doc["paths"]["/api/v1/active/storage/uploads/{upload_id}"]["post"];
    assert_eq!(upload["parameters"][0]["required"], true);
    assert!(upload["requestBody"]["content"]["application/octet-stream"].is_object());
    assert_eq!(openapi(None)["servers"][0]["url"], "/");
}
#[tokio::test]
async fn response_contracts_validate_real_services_with_mock_hardware() {
    let (_dir, mut app, _) = crate::input_api::tests::setup();
    crate::tty::tests::install(&mut app);
    crate::storage::tests::install(&mut app);
    crate::network::manager::tests::install(&mut app);
    app.remote.write().rest = Some("127.0.0.1:21981".parse().unwrap());
    let owner = crate::input_sessions::Principal {
        key_id: "docs".into(),
        client_id: "mock".into(),
        cancelled: Default::default(),
        ended: Default::default(),
    };
    for name in [
        "app_get_state",
        "events_get_snapshot",
        "channels_list",
        "channel_get_active",
        "video_get_status",
        "hid_get_status",
        "storage_get_status",
        "network_get_status",
        "wake_get_status",
        "logs_get_state",
        "diagnostics_get",
        "network_configure",
        "network_forward_configure",
        "network_set_mode",
        "wake_configure",
        "wake_send",
        "logs_configure",
        "logs_flush",
        "tty_open",
        "tty_get_status",
        "tty_read",
        "tty_write",
        "tty_close",
        "hid_key_down",
        "hid_key_up",
        "hid_button_down",
        "hid_button_up",
        "hid_mouse_move",
        "hid_wheel",
        "hid_chord",
        "hid_click",
        "hid_double_click",
        "hid_key_sequence",
        "hid_release",
        "hid_recover",
        "storage_upload_prepare",
    ] {
        let mut args = example_args(name);
        if name == "network_forward_configure" {
            args = json!({"rules":[]});
        }
        let result = app
            .execute_remote(&owner, name, args)
            .await
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let schema = response_schema(name).unwrap();
        let v = jsonschema::validator_for(&schema).unwrap();
        assert!(
            v.is_valid(&result),
            "{name}: {}",
            v.iter_errors(&result)
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    let session = app
        .execute_remote(&owner, "input_open", json!({}))
        .await
        .unwrap();
    valid(&response_schema("input_open").unwrap(), &session);
    for name in ["input_heartbeat", "input_close"] {
        let value = app
            .execute_remote(
                &owner,
                name,
                json!({"input_session":session["input_session"]}),
            )
            .await
            .unwrap();
        valid(&response_schema(name).unwrap(), &value);
    }
    let started = app
        .execute_remote(&owner, "storage_action", json!({"action":"refresh"}))
        .await
        .unwrap();
    valid(&response_schema("storage_action").unwrap(), &started);
    let job = app
        .execute_remote(
            &owner,
            "storage_job_get",
            json!({"job_id":started["job_id"]}),
        )
        .await
        .unwrap();
    valid(&response_schema("storage_job_get").unwrap(), &job);
    valid(
        &error_schema(),
        &json!(crate::remote_error::Context::new()
            .failure(AppError::new("NO_ACTIVE_CHANNEL", "no active"))),
    );
    valid(
        &error_schema(),
        &json!(crate::remote_error::Context::new()
            .failure(AppError::uncertain("TIMEOUT", "uncertain"))),
    );
    app.shutdown().await;
}
#[test]
fn reference_search_covers_titles_paths_methods_groups_and_names() {
    let c = crate::service::COMMANDS
        .iter()
        .find(|c| c.name == "hid_key_down")
        .unwrap();
    for query in [
        "HID_KEY_DOWN",
        "post",
        "/keys/down",
        "키 누르기",
        "키보드·마우스",
        " ",
    ] {
        assert!(matches(c, query), "{query}");
    }
    assert!(!matches(c, "missing-command"));
}

#[test]
fn guide_and_examples_cover_all_tools_without_real_credentials_or_fake_bound_addresses() {
    let guide = mcp_guide(None, None);
    assert!(guide.contains("REST: OFF"));
    assert!(guide.contains("<API_KEY>"));
    for command in crate::service::COMMANDS {
        assert!(guide.contains(command.name), "{}", command.name);
    }
    for resource in crate::remote_resources::RESOURCES {
        assert!(guide.contains(resource.uri));
    }
    let configured = mcp_config(Some("127.0.0.1:24981".parse().unwrap()));
    assert_eq!(configured["url"], "http://127.0.0.1:24981/mcp");
    for command in crate::service::COMMANDS {
        let example =
            crate::remote::request_example(command, Some("127.0.0.1:24980".parse().unwrap()));
        assert!(example.contains("http://127.0.0.1:24980"));
        assert!(example.contains("Bearer <API_KEY>"));
    }
}
