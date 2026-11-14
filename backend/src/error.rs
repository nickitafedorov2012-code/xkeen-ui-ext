use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::fmt;

/// Единый тип ошибок API с маппингом в HTTP-статусы (400 / 502 / 503 / 500)
/// и сохранением стандартного JSON-контракта `{ "success": false, "error": "..." }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    BadRequest(String),
    BadGateway(String),
    ServiceUnavailable(String),
    Internal(String),
}

pub type ApiResult<T> = Result<T, AppError>;

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn bad_gateway(msg: impl Into<String>) -> Self {
        Self::BadGateway(msg.into())
    }

    pub fn service_unavailable(msg: impl Into<String>) -> Self {
        Self::ServiceUnavailable(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::BadGateway(_) => StatusCode::BAD_GATEWAY,
            Self::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest(m)
            | Self::BadGateway(m)
            | Self::ServiceUnavailable(m)
            | Self::Internal(m) => m.as_str(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for AppError {}

impl From<String> for AppError {
    fn from(value: String) -> Self {
        Self::Internal(value)
    }
}

impl From<&str> for AppError {
    fn from(value: &str) -> Self {
        Self::Internal(value.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::Internal(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::BadRequest(err.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let message = self.message().to_string();
        (status, Json(json!({ "success": false, "error": message }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn decode_response(err: AppError) -> (StatusCode, serde_json::Value) {
        let resp = err.into_response();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 16 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn app_error_maps_to_expected_http_status_and_json_contract() {
        let (st, body) = decode_response(AppError::bad_request("invalid port")).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({ "success": false, "error": "invalid port" }));

        let (st, body) = decode_response(AppError::bad_gateway("mihomo timeout")).await;
        assert_eq!(st, StatusCode::BAD_GATEWAY);
        assert_eq!(body, json!({ "success": false, "error": "mihomo timeout" }));

        let (st, body) = decode_response(AppError::service_unavailable("adguard down")).await;
        assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, json!({ "success": false, "error": "adguard down" }));

        let (st, body) = decode_response(AppError::internal("disk full")).await;
        assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body, json!({ "success": false, "error": "disk full" }));
    }
}
