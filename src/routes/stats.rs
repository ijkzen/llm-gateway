use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use chrono::{Datelike, Offset, TimeZone};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::app_settings::AppSettings;
use crate::response::{self, Response};
use crate::state::AppState;

mod compute;
mod insight;
mod metrics;
mod rank;
mod rank_impl;
mod rank_snap;
mod summary_charts;
mod window;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/summary", get(summary_charts::summary))
        .route("/charts", get(summary_charts::charts))
        .route("/provider-rank", get(rank_impl::provider_rank))
        .route("/virtual-model-rank", get(rank_impl::virtual_model_rank))
        .route("/provider-model-rank", get(rank_impl::provider_model_rank))
        .route(
            "/virtual-model-member-rank",
            get(rank_impl::virtual_model_member_rank),
        )
        .route("/api-key-rank", get(rank_impl::api_key_rank))
        .route("/model-metrics", get(metrics::model_metrics))
        .route("/api-key-metrics", get(metrics::api_key_metrics))
        .route("/provider-metrics", get(metrics::provider_metrics))
        .route(
            "/virtual-model-metrics",
            get(metrics::virtual_model_metrics),
        )
        .route("/insight", get(insight::insight))
}

#[cfg(test)]
mod tests;
pub(crate) use compute::percentile;
pub(crate) use compute::required_time_range;
pub(crate) use compute::round_5;
pub(crate) use compute::weighted_ratio;
pub(crate) use summary_charts::ChartsQuery;
pub(crate) use summary_charts::FloatTrendPoint;
pub(crate) use summary_charts::TrendPoint;
pub(crate) use window::DAY_MS;
pub(crate) use window::Granularity;
pub(crate) use window::HOUR_MS;
#[cfg(test)]
pub(crate) use window::merge_natural_periods;
pub(crate) use window::resolve_chart_window;
pub(crate) use window::stats_tz_offset_minutes;

#[allow(unused_imports)]
pub(crate) use window::ChartWindow;
#[allow(unused_imports)]
pub(crate) use window::tz_offset_minutes_at;
