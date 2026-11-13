// Apifox open-API client: imports an OpenAPI document into one Apifox project.
//
// The access token never reaches this code: the Authorization header carries
// `{{secret:apifox_token}}`, which the IDE fills in on the way out.

use openidea_plugin::{host, Error, HttpRequest};
use serde_json::{json, Value};

/// Which behaviour to use when Apifox already has an endpoint at the same path.
/// AUTO_MERGE keeps manual edits made in Apifox; see the plugin manifest note.
pub const OVERWRITE_BEHAVIOUR: &str = "AUTO_MERGE";

pub struct ImportOutcome {
    pub created: u64,
    pub updated: u64,
    pub failed: u64,
    pub ignored: u64,
    /// Messages from `data.errors` (partial failures, "import in progress", ...).
    pub errors: Vec<String>,
}

impl ImportOutcome {
    /// One-line Chinese summary for notifications and the command result.
    pub fn summary(&self) -> String {
        format!(
            "新增 {} 个接口，更新 {} 个，忽略 {} 个，失败 {} 个",
            self.created, self.updated, self.ignored, self.failed
        )
    }
}

/// `POST /v1/projects/{projectId}/import-openapi` with a raw OpenAPI JSON string.
pub fn import(project_id: &str, openapi_json: &str) -> Result<ImportOutcome, Error> {
    let url = format!(
        "https://api.apifox.com/v1/projects/{}/import-openapi?locale=zh-CN",
        project_id
    );
    let body = json!({
        "input": openapi_json,
        "options": {
            "endpointOverwriteBehavior": OVERWRITE_BEHAVIOUR,
            "schemaOverwriteBehavior": OVERWRITE_BEHAVIOUR,
            "prependBasePath": false
        }
    })
    .to_string();

    let mut req = HttpRequest::post(&url, body)
        .header("X-Apifox-Api-Version", "2024-03-28")
        .header("Authorization", "Bearer {{secret:apifox_token}}")
        .header("Content-Type", "application/json");
    req.timeout_ms = Some(60_000);

    let resp = host::fetch(&req)?;
    let text = resp.text();
    if !(200..300).contains(&resp.status) {
        return Err(format!("Apifox 返回 HTTP {}：{}", resp.status, truncate(&text, 600)).into());
    }

    let value: Value = serde_json::from_str(&text)
        .map_err(|e| format!("无法解析 Apifox 响应（{e}）：{}", truncate(&text, 600)))?;
    let data = &value["data"];
    let counters = &data["counters"];
    let count = |key: &str| counters[key].as_u64().unwrap_or(0);

    let mut errors = Vec::new();
    if let Some(list) = data["errors"].as_array() {
        for item in list {
            let message = item["message"].as_str().unwrap_or("").trim();
            let code = item["code"].as_str().unwrap_or("").trim();
            if message.is_empty() {
                continue;
            }
            errors.push(if code.is_empty() {
                message.to_string()
            } else {
                format!("[{code}] {message}")
            });
        }
    }

    Ok(ImportOutcome {
        created: count("endpointCreated"),
        updated: count("endpointUpdated"),
        failed: count("endpointFailed"),
        ignored: count("endpointIgnored"),
        errors,
    })
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    format!("{head}…（已截断）")
}
