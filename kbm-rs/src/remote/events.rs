use super::*;
use axum::response::sse::{Event, KeepAlive, Sse};
use parking_lot::Mutex;
use std::{collections::BTreeMap, convert::Infallible, time::Duration};

pub(super) async fn stream(
    State(app): State<Arc<AppService>>,
    Extension(owner): Extension<crate::input_sessions::Principal>,
    OriginalUri(uri): OriginalUri,
) -> Result<Response, AppError> {
    no_query(&uri)?;
    let sub = app.events.subscribe(&app, &owner)?;
    let stream = futures_util::stream::unfold(sub, |mut sub| async move {
        let frame = sub.next().await?;
        let event = Event::default()
            .id(&frame.id)
            .event(frame.event)
            .data(&frame.json);
        Some((Ok::<_, Infallible>(event), sub))
    });
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response())
}
pub(super) async fn snapshot(
    State(app): State<Arc<AppService>>,
    Extension(owner): Extension<crate::input_sessions::Principal>,
    OriginalUri(uri): OriginalUri,
) -> Result<Json<Value>, AppError> {
    no_query(&uri)?;
    checked(&owner)?;
    let result = app
        .execute_remote(&owner, "events_get_snapshot", json!({}))
        .await?;
    checked(&owner)?;
    Ok(Json(result))
}
fn no_query(uri: &axum::http::Uri) -> Result<(), AppError> {
    if uri.query().is_some_and(|q| !q.is_empty()) {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "이벤트는 UID나 query 인자를 받지 않습니다.",
        ));
    }
    Ok(())
}
fn checked(owner: &crate::input_sessions::Principal) -> Result<(), AppError> {
    if owner.cancelled.is_cancelled() || owner.ended.is_cancelled() {
        return Err(AppError::new(
            "UNAUTHORIZED",
            "인증 또는 연결이 종료됐습니다.",
        ));
    }
    Ok(())
}
fn owner(
    context: &RequestContext<RoleServer>,
) -> Result<crate::input_sessions::Principal, ErrorData> {
    context
        .extensions
        .get::<axum::http::request::Parts>()
        .and_then(|p| p.extensions.get::<crate::input_sessions::Principal>())
        .cloned()
        .ok_or_else(|| {
            failure(
                context,
                AppError::new("UNAUTHORIZED", "인증된 연결이 필요합니다."),
            )
        })
        .and_then(|p| {
            checked(&p).map_err(|e| failure(context, e))?;
            Ok(p)
        })
}
fn failure(context: &RequestContext<RoleServer>, error: AppError) -> ErrorData {
    ErrorData::invalid_params(
        error.message.clone(),
        Some(json!(error_context(context).failure(error))),
    )
}
pub(super) fn live(uri: &str) -> bool {
    [
        "minikmb://app/state",
        "minikmb://channels",
        "minikmb://active/channel",
        "minikmb://active/diagnostics",
        "minikmb://events/snapshot",
    ]
    .contains(&uri)
}

/// One instance per MCP session. No unbounded task/notification queue; duplicate
/// URI subscription is idempotent and unsubscribe joins before acknowledging.
#[derive(Default)]
pub(super) struct Subscriptions(Mutex<BTreeMap<String, tokio::task::JoinHandle<()>>>);
impl Drop for Subscriptions {
    fn drop(&mut self) {
        for (_, task) in std::mem::take(self.0.get_mut()) {
            task.abort();
        }
    }
}
impl Mcp {
    pub(super) async fn subscribe_events(
        &self,
        uri: String,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let p = owner(&context)?;
        if !live(&uri) {
            return Err(failure(
                &context,
                AppError::new(
                    "INVALID_ARGUMENTS",
                    "이 URI는 변경 구독을 지원하지 않습니다.",
                ),
            ));
        }
        let mut tasks = self.subscriptions.0.lock();
        tasks.retain(|_, t| !t.is_finished());
        if tasks.contains_key(&uri) {
            return Ok(());
        }
        let mut sub = self
            .app
            .events
            .subscribe(&self.app, &p)
            .map_err(|e| failure(&context, e))?;
        let peer = context.peer.clone();
        tasks.insert(
            uri.clone(),
            tokio::spawn(async move {
                // One URI-only invalidation per observed latest state. Consumers
                // re-read; intermediate ticks may coalesce, including short jobs.
                while sub.next().await.is_some() {
                    if tokio::time::timeout(
                        Duration::from_secs(1),
                        peer.notify_resource_updated(
                            rmcp::model::ResourceUpdatedNotificationParam::new(uri.clone()),
                        ),
                    )
                    .await
                    .map_or(true, |r| r.is_err())
                    {
                        break;
                    }
                }
            }),
        );
        Ok(())
    }
    pub(super) async fn unsubscribe_events(
        &self,
        uri: String,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let _ = owner(&context)?;
        if !live(&uri) {
            return Err(failure(
                &context,
                AppError::new(
                    "INVALID_ARGUMENTS",
                    "이 URI는 변경 구독을 지원하지 않습니다.",
                ),
            ));
        }
        let task = self.subscriptions.0.lock().remove(&uri);
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
        Ok(())
    }
    pub(super) async fn listen_events(
        &self,
        context: rmcp::service::SubscriptionContext,
    ) -> Result<(), ErrorData> {
        let p = owner(context.request_context())?;
        let uris = context
            .accepted()
            .resource_subscriptions
            .clone()
            .unwrap_or_default();
        if uris.is_empty() {
            return Ok(());
        }
        let mut sub = self
            .app
            .events
            .subscribe(&self.app, &p)
            .map_err(|e| failure(context.request_context(), e))?;
        loop {
            tokio::select! {
                biased;
                _=context.cancelled()=>break,
                frame=sub.next()=>{
                    if frame.is_none() {break;}
                    for uri in &uris {
                        tokio::select! {
                            biased;
                            _=context.cancelled()=>return Ok(()),
                            _=p.cancelled.cancelled()=>return Ok(()),
                            _=p.ended.cancelled()=>return Ok(()),
                            result=tokio::time::timeout(Duration::from_secs(1),context.sink().notify_resource_updated(uri.clone()))=>{
                                if result.map_or(true,|r|r.is_err()) {return Ok(());}
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
