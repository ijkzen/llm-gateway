use axum::Router;
use axum::response::IntoResponse;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

pub fn apply<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(CatchPanicLayer::custom(|_| {
            let msg = crate::app_settings::AppSettings::lang_sync()
                .tr("服务器内部错误", "internal server error");
            crate::response::internal_error::<String>(msg).into_response()
        }))
}
