use super::*;
#[test]
fn command_types_reject_routing_overrides_and_invalid_parameters() {
    for (name, args) in [
        ("network_get_status", json!({})),
        ("network_set_mode", json!({"mode":"emulation"})),
        ("network_configure", json!(Config::default())),
        (
            "network_scan_start",
            json!({"target":"192.168.127.100","start":1,"end":1024,"timeout_ms":1000}),
        ),
        ("network_scan_stop", json!({})),
        ("network_forward_start", json!({"rules":[]})),
        ("network_forward_configure", json!({"rules":[]})),
        ("network_forward_stop", json!({})),
    ] {
        assert_eq!(schema(name).unwrap()["additionalProperties"], false);
        for field in ["uid", "uids", "generation", "port", "interface"] {
            let mut bad = args.clone();
            bad[field] = json!([]);
            let valid = match name {
                "network_get_status" | "network_scan_stop" | "network_forward_stop" => {
                    serde_json::from_value::<Empty>(bad).is_ok()
                }
                "network_set_mode" => serde_json::from_value::<ModeArgs>(bad).is_ok(),
                "network_scan_start" => {
                    serde_json::from_value::<crate::network::scanner::Request>(bad).is_ok()
                }
                "network_forward_start" | "network_forward_configure" => {
                    serde_json::from_value::<ForwardArgs>(bad).is_ok()
                }
                _ => serde_json::from_value::<Config>(bad).is_ok(),
            };
            assert!(!valid, "{name}/{field}");
        }
    }
    assert!(serde_json::from_value::<ModeArgs>(json!({"mode":"both"})).is_err());
    let rule = json!({"id":"ssh","protocol":"tcp","listen_port":18022,"target_ip":"192.168.127.100","target_port":22});
    for field in ["uid", "uids", "port", "interface", "host"] {
        let mut bad = rule.clone();
        bad[field] = json!(0);
        assert!(serde_json::from_value::<ForwardArgs>(json!({"rules":[bad]})).is_err());
    }
}
#[tokio::test]
async fn no_active_channel_is_preserved_for_every_network_command() {
    let (_dir, mut app, _) = crate::input_api::tests::setup();
    crate::network::manager::tests::install(&mut app);
    app.select(None).await.unwrap();
    let owner = Principal {
        key_id: "test".into(),
        client_id: "test".into(),
        cancelled: Default::default(),
        ended: Default::default(),
    };
    for (name, args) in [
        ("network_get_status", json!({})),
        ("network_set_mode", json!({"mode":"off"})),
        ("network_configure", json!(Config::default())),
        (
            "network_scan_start",
            json!({"target":"192.168.127.100","start":1,"end":1024,"timeout_ms":1000}),
        ),
        ("network_scan_stop", json!({})),
        ("network_forward_start", json!({"rules":[]})),
        ("network_forward_configure", json!({"rules":[]})),
        ("network_forward_stop", json!({})),
    ] {
        assert_eq!(
            app.execute_remote(&owner, name, args)
                .await
                .err()
                .unwrap()
                .code,
            "NO_ACTIVE_CHANNEL"
        );
    }
}
