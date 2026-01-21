use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// JMAP Request (RFC 8620 Section 3.3)
#[derive(Serialize, Deserialize, Debug)]
pub struct JmapRequest {
    pub using: Vec<String>,
    #[serde(rename = "methodCalls")]
    pub method_calls: Vec<MethodCall>,
    #[serde(rename = "createdIds", skip_serializing_if = "Option::is_none")]
    pub created_ids: Option<HashMap<String, String>>,
}

/// Method call tuple: [name, arguments, callId]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum MethodCall {
    Array(Vec<serde_json::Value>),
}

impl MethodCall {
    pub fn name(&self) -> Option<&str> {
        match self {
            MethodCall::Array(arr) => arr.get(0)?.as_str(),
        }
    }

    pub fn arguments(&self) -> Option<&serde_json::Value> {
        match self {
            MethodCall::Array(arr) => arr.get(1),
        }
    }

    pub fn call_id(&self) -> Option<&str> {
        match self {
            MethodCall::Array(arr) => arr.get(2)?.as_str(),
        }
    }
}

/// JMAP Response (RFC 8620 Section 3.4)
#[derive(Serialize, Deserialize, Debug)]
pub struct JmapResponse {
    #[serde(rename = "methodResponses")]
    pub method_responses: Vec<MethodResponse>,
    #[serde(rename = "createdIds", skip_serializing_if = "Option::is_none")]
    pub created_ids: Option<HashMap<String, String>>,
    #[serde(rename = "sessionState")]
    pub session_state: String,
}

/// Method response tuple: [name, arguments, callId]
pub type MethodResponse = Vec<serde_json::Value>;

/// Standard JMAP Error
#[derive(Serialize, Deserialize, Debug)]
pub struct JmapError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub status: Option<u16>,
    pub detail: Option<String>,
}

/// Mailbox object (RFC 8621 Section 2)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Mailbox {
    pub id: String,
    pub name: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub role: Option<String>,
    #[serde(rename = "sortOrder")]
    pub sort_order: u32,
    #[serde(rename = "totalEmails")]
    pub total_emails: u64,
    #[serde(rename = "unreadEmails")]
    pub unread_emails: u64,
    #[serde(rename = "totalThreads")]
    pub total_threads: u64,
    #[serde(rename = "unreadThreads")]
    pub unread_threads: u64,
    #[serde(rename = "myRights")]
    pub my_rights: MailboxRights,
    #[serde(rename = "isSubscribed")]
    pub is_subscribed: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MailboxRights {
    #[serde(rename = "mayReadItems")]
    pub may_read_items: bool,
    #[serde(rename = "mayAddItems")]
    pub may_add_items: bool,
    #[serde(rename = "mayRemoveItems")]
    pub may_remove_items: bool,
    #[serde(rename = "maySetSeen")]
    pub may_set_seen: bool,
    #[serde(rename = "maySetKeywords")]
    pub may_set_keywords: bool,
    #[serde(rename = "mayCreateChild")]
    pub may_create_child: bool,
    #[serde(rename = "mayRename")]
    pub may_rename: bool,
    #[serde(rename = "mayDelete")]
    pub may_delete: bool,
    #[serde(rename = "maySubmit")]
    pub may_submit: bool,
}

impl Default for MailboxRights {
    fn default() -> Self {
        Self {
            may_read_items: true,
            may_add_items: true,
            may_remove_items: true,
            may_set_seen: true,
            may_set_keywords: true,
            may_create_child: true,
            may_rename: true,
            may_delete: true,
            may_submit: true,
        }
    }
}

/// Email object (simplified version of RFC 8621 Section 4)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Email {
    pub id: String,
    #[serde(rename = "blobId")]
    pub blob_id: String,
    #[serde(rename = "threadId")]
    pub thread_id: String,
    #[serde(rename = "mailboxIds")]
    pub mailbox_ids: HashMap<String, bool>,
    pub keywords: HashMap<String, bool>,
    pub size: u64,
    #[serde(rename = "receivedAt")]
    pub received_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<Vec<EmailAddress>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<Vec<EmailAddress>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EmailAddress {
    pub name: Option<String>,
    pub email: String,
}

/// Get request arguments
#[derive(Serialize, Deserialize, Debug)]
pub struct GetRequest {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Vec<String>>,
}

/// Get response
#[derive(Serialize, Deserialize, Debug)]
pub struct GetResponse<T> {
    #[serde(rename = "accountId")]
    pub account_id: String,
    pub state: String,
    pub list: Vec<T>,
    #[serde(rename = "notFound")]
    pub not_found: Vec<String>,
}
