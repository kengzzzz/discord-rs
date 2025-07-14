use serde::{Deserialize, Serialize};

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
        }
    }
}
