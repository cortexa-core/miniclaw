use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use super::http::HttpState;

pub async fn get_config(
    State(state): State<Arc<HttpState>>,
) -> impl IntoResponse {
    match crate::config::Config::load(&state.config_path) {
        Ok(config) => {
            let mut json = serde_json::to_value(&config).unwrap_or_default();
            mask_api_keys(&mut json);
            (StatusCode::OK, Json(json))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to load config: {e}")})),
        ),
    }
}

pub async fn post_config(
    State(state): State<Arc<HttpState>>,
    Json(new_config): Json<serde_json::Value>,
) -> impl IntoResponse {
    let toml_str = match json_config_to_toml(&new_config) {
        Ok(t) => t,
        Err(e) => return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Invalid config: {e}")})),
        ),
    };

    if let Err(e) = toml::from_str::<crate::config::Config>(&toml_str) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Config validation failed: {e}")})),
        );
    }

    if let Err(e) = tokio::fs::write(&state.config_path, &toml_str).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to write config: {e}")})),
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "saved", "message": "Config saved. Restart agent to apply changes."})),
    )
}

fn mask_api_keys(value: &mut serde_json::Value) {
    if let Some(obj) = value.as_object_mut() {
        for (key, val) in obj.iter_mut() {
            if key.contains("api_key") && !key.contains("env") {
                if let Some(s) = val.as_str() {
                    if !s.is_empty() {
                        *val = serde_json::json!("••••••••");
                    }
                }
            }
            mask_api_keys(val);
        }
    }
    if let Some(arr) = value.as_array_mut() {
        for item in arr {
            mask_api_keys(item);
        }
    }
}

fn json_config_to_toml(json: &serde_json::Value) -> Result<String, String> {
    let config: crate::config::Config = serde_json::from_value(json.clone())
        .map_err(|e| format!("Invalid config structure: {e}"))?;
    toml::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize to TOML: {e}"))
}
