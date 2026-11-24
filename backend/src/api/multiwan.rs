use axum::{extract::State, response::Response, Json};
use serde::Deserialize;

use crate::{
    api::{api_err, api_ok},
    config::{MultiWanChannelConfig, MultiWanWeightMode},
    AppState,
};

#[derive(Debug, Deserialize)]
pub struct MultiWanConfigReq {
    pub enabled: Option<bool>,
    pub mode: Option<MultiWanWeightMode>,
    pub single_ip_subscriptions: Option<bool>,
    pub channels: Option<Vec<MultiWanChannelConfig>>,
}

/// GET /api/multiwan/status
pub async fn get_multiwan_status(State(state): State<AppState>) -> Response {
    let mut st = state.multiwan.status().await;
    if st.last_reconcile_ts == 0 {
        st = state.multiwan.reconcile_now(&state).await;
    }
    api_ok(serde_json::json!(st))
}

/// POST /api/multiwan/config
pub async fn post_multiwan_config(
    State(state): State<AppState>,
    Json(req): Json<MultiWanConfigReq>,
) -> Response {
    let res = state
        .update_config(|cfg| {
            if let Some(enabled) = req.enabled {
                cfg.multiwan.enabled = enabled;
            }
            if let Some(mode) = req.mode {
                cfg.multiwan.mode = mode;
            }
            if let Some(single_ip) = req.single_ip_subscriptions {
                cfg.multiwan.single_ip_subscriptions = single_ip;
            }
            if let Some(channels) = req.channels {
                cfg.multiwan.channels = channels
                    .into_iter()
                    .map(|mut c| {
                        c.weight = c.weight.min(100);
                        c
                    })
                    .collect();
            }
            Ok::<(), String>(())
        })
        .await;

    if let Err(e) = res {
        return api_err(e);
    }
    state.multiwan.trigger_reconcile();
    let mut st = state.multiwan.status().await;
    let cfg = state.cfg_snapshot().await;
    st.enabled = cfg.multiwan.enabled;
    st.mode = cfg.multiwan.mode;
    st.single_ip_subscriptions = cfg.multiwan.single_ip_subscriptions;
    api_ok(serde_json::json!(st))
}

/// POST /api/multiwan/reconcile
pub async fn post_multiwan_reconcile(State(state): State<AppState>) -> Response {
    let st = state.multiwan.reconcile_now(&state).await;
    api_ok(serde_json::json!(st))
}

/// POST /api/multiwan/test-probe
pub async fn post_multiwan_test_probe(State(state): State<AppState>) -> Response {
    let st = state.multiwan.run_probe_now(&state).await;
    api_ok(serde_json::json!(st))
}
