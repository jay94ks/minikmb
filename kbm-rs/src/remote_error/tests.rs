use super::*;
fn binding(n: u8, generation: u64) -> Binding {
    Binding {
        uid: Uid::from_bytes(&[n; 12]).unwrap(),
        generation,
    }
}
#[test]
fn accepted_error_origin_wins_over_later_channel_and_is_not_persisted_in_core_errors() {
    let context = Context::new();
    context.observe(binding(2, 9));
    let error =
        AppError::uncertain("HID_RELEASE_UNCONFIRMED", "release unknown").at(&binding(1, 7));
    assert!(serde_json::to_value(&error)
        .unwrap()
        .get("origin")
        .is_none());
    let failure = context.failure(error);
    assert_eq!(failure.channel_uid, Some(binding(1, 7).uid));
    assert_eq!(failure.generation, Some(7));
    assert!(failure.error.uncertain);
}
#[test]
fn revoked_credentials_never_disclose_even_a_previously_known_job_source() {
    let context = Context::new();
    context.observe(binding(2, 9));
    let failure = context
        .failure(AppError::new("UNAUTHORIZED", "revoked").at_job(&binding(1, 7), "verified-job"));
    assert!(
        failure.channel_uid.is_none() && failure.generation.is_none() && failure.job_id.is_none()
    );
    assert_eq!(failure.request_id, context.request_id);
}
#[test]
fn correlation_ids_are_unique_and_limit_and_conflict_errors_have_distinct_statuses() {
    let ids: std::collections::HashSet<_> = (0..1000).map(|_| Context::new().request_id).collect();
    assert_eq!(ids.len(), 1000);
    for code in ["INPUT_QUEUE_FULL", "INPUT_SESSION_LIMIT", "CAPTURE_BUSY"] {
        assert_eq!(
            status(&AppError::new(code, "limit")),
            StatusCode::TOO_MANY_REQUESTS
        );
    }
    for code in ["JOB_NOT_CANCELLABLE", "STALE_SESSION", "NETWORK_BUSY"] {
        assert_eq!(
            status(&AppError::new(code, "conflict")),
            StatusCode::CONFLICT
        );
    }
}
#[tokio::test]
async fn rest_rejections_are_redacted_and_success_payload_is_not_rewritten() {
    let context = Context::new();
    let response = context.finish(
        (StatusCode::BAD_REQUEST, "secret parser diagnostic").into_response(),
        true,
    );
    assert_eq!(
        response.headers()["x-minikmb-request-id"],
        context.request_id
    );
    let value: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 16384)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["code"], "INVALID_ARGUMENTS");
    assert!(!value.to_string().contains("secret"));
    let response = context.finish(
        Json(serde_json::json!({"accepted":true})).into_response(),
        true,
    );
    let value: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 16384)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value, serde_json::json!({"accepted":true}));
}

#[tokio::test]
async fn revocation_before_finalization_returns_only_a_typed_unauthorized_failure() {
    let context = Context::new();
    context.observe(binding(1, 7));
    let token = tokio_util::sync::CancellationToken::new();
    context.authorize(token.clone());
    token.cancel();
    let response = context.finish(
        Json(serde_json::json!({"secret":"not public"})).into_response(),
        true,
    );
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let value: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 16384)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["code"], "UNAUTHORIZED");
    assert_eq!(value["uncertain"], true);
    assert!(value["channel_uid"].is_null());
    assert!(!value.to_string().contains("not public"));
}
#[tokio::test]
async fn final_error_body_is_also_stopped_if_the_key_is_revoked_before_body_delivery() {
    let context = Context::new();
    context.observe(binding(1, 7));
    let token = tokio_util::sync::CancellationToken::new();
    context.authorize(token.clone());
    let response = context.finish(
        AppError::uncertain("HID_RELEASE_UNCONFIRMED", "unknown")
            .at(&binding(1, 7))
            .into_response(),
        true,
    );
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response.headers()["cache-control"], "no-store");
    token.cancel();
    assert!(axum::body::to_bytes(response.into_body(), 16384)
        .await
        .unwrap()
        .is_empty());
}
