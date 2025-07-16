use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

fn utc_now() -> DateTime<Utc> {
    Utc::now()
}

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct ChatEntry {
    pub role: String,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default)]
    pub ref_text: Option<String>,
    #[serde(default)]
    pub ref_attachments: Option<Vec<String>>,
    #[serde(default)]
    pub ref_author: Option<String>,
    #[serde(default = "utc_now", with = "chrono::serde::ts_seconds")]
    pub created_at: DateTime<Utc>,
}

impl ChatEntry {
    pub fn new(
        role: String,
        text: String,
        attachments: Vec<String>,
        ref_text: Option<String>,
        ref_attachments: Option<Vec<String>>,
        ref_author: Option<String>,
    ) -> Self {
        Self {
            role,
            text,
            attachments,
            ref_text,
            ref_attachments,
            ref_author,
            created_at: Utc::now(),
        }
    }
}
