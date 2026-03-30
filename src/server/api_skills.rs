use axum::{extract::State, Json};
use std::sync::Arc;

use super::http::HttpState;
use crate::agent::skills::SkillManager;

pub async fn get_skills(State(state): State<Arc<HttpState>>) -> Json<serde_json::Value> {
    let skills_dir = state.data_dir.join("skills");
    let mgr = SkillManager::load(&skills_dir, &[]);
    let metadata = mgr.skills_metadata();
    Json(serde_json::json!({
        "count": metadata.len(),
        "skills": metadata,
    }))
}
