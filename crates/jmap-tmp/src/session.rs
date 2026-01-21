use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// JMAP Session resource (RFC 8620 Section 2)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JmapSession {
    pub capabilities: HashMap<String, serde_json::Value>,
    pub accounts: HashMap<String, Account>,
    #[serde(rename = "primaryAccounts")]
    pub primary_accounts: HashMap<String, String>,
    pub username: String,
    #[serde(rename = "apiUrl")]
    pub api_url: String,
    #[serde(rename = "downloadUrl")]
    pub download_url: String,
    #[serde(rename = "uploadUrl")]
    pub upload_url: String,
    #[serde(rename = "eventSourceUrl")]
    pub event_source_url: String,
    pub state: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Account {
    pub name: String,
    #[serde(rename = "isPersonal")]
    pub is_personal: bool,
    #[serde(rename = "isReadOnly")]
    pub is_read_only: bool,
    #[serde(rename = "accountCapabilities")]
    pub account_capabilities: HashMap<String, serde_json::Value>,
}

impl JmapSession {
    pub fn new() -> Self {
        let account_id = "u1234567890";
        
        // Core capabilities
        let mut capabilities = HashMap::new();
        capabilities.insert(
            "urn:ietf:params:jmap:core".to_string(),
            serde_json::json!({
                "maxSizeUpload": 50000000,
                "maxConcurrentUpload": 4,
                "maxSizeRequest": 10000000,
                "maxConcurrentRequests": 4,
                "maxCallsInRequest": 16,
                "maxObjectsInGet": 500,
                "maxObjectsInSet": 500,
                "collationAlgorithms": [
                    "i;ascii-numeric",
                    "i;ascii-casemap",
                    "i;unicode-casemap"
                ]
            }),
        );

        // Mail capabilities
        capabilities.insert(
            "urn:ietf:params:jmap:mail".to_string(),
            serde_json::json!({
                "maxMailboxesPerEmail": null,
                "maxMailboxDepth": null,
                "maxSizeMailboxName": 256,
                "maxSizeAttachmentsPerEmail": 50000000,
                "emailQuerySortOptions": ["receivedAt", "sentAt", "size", "from", "to", "subject"],
                "mayCreateTopLevelMailbox": true
            }),
        );

        // Submission capability
        capabilities.insert(
            "urn:ietf:params:jmap:submission".to_string(),
            serde_json::json!({
                "maxDelayedSend": 86400,
                "submissionExtensions": []
            }),
        );

        // Create account
        let mut accounts = HashMap::new();
        let mut account_capabilities = HashMap::new();
        account_capabilities.insert(
            "urn:ietf:params:jmap:core".to_string(),
            serde_json::json!({}),
        );
        account_capabilities.insert(
            "urn:ietf:params:jmap:mail".to_string(),
            serde_json::json!({}),
        );
        account_capabilities.insert(
            "urn:ietf:params:jmap:submission".to_string(),
            serde_json::json!({}),
        );

        accounts.insert(
            account_id.to_string(),
            Account {
                name: "user@example.com".to_string(),
                is_personal: true,
                is_read_only: false,
                account_capabilities,
            },
        );

        // Primary accounts
        let mut primary_accounts = HashMap::new();
        primary_accounts.insert("urn:ietf:params:jmap:mail".to_string(), account_id.to_string());
        primary_accounts.insert("urn:ietf:params:jmap:submission".to_string(), account_id.to_string());

        Self {
            capabilities,
            accounts,
            primary_accounts,
            username: "user@example.com".to_string(),
            api_url: "/jmap".to_string(),
            download_url: "/download/{accountId}/{blobId}/{name}".to_string(),
            upload_url: "/upload/{accountId}".to_string(),
            event_source_url: "/eventsource".to_string(),
            state: "state-001".to_string(),
        }
    }
}
