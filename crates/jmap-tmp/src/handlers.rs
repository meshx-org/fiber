use crate::session::JmapSession;
use crate::types::*;
use std::collections::HashMap;
use wstd::http::{Body, Request, Response, StatusCode};

/// Session endpoint (RFC 8620 Section 2)
pub async fn session(_req: Request<Body>) -> Result<Response<Body>, wstd::http::Error> {
    let session = JmapSession::new();
    let json = serde_json::to_string(&session).unwrap();

    Ok(Response::builder()
        .header("Content-Type", "application/json")
        .body(json.into())
        .unwrap())
}

/// Main JMAP API endpoint (RFC 8620 Section 3)
pub async fn jmap_api(req: Request<Body>) -> Result<Response<Body>, wstd::http::Error> {
    // Read request body
    let mut body = req.into_body();
    let body_bytes: &[u8] = body.contents().await?;
    let body_str = String::from_utf8_lossy(body_bytes);

    // Parse JMAP request
    let jmap_request: JmapRequest = match serde_json::from_str(&body_str) {
        Ok(req) => req,
        Err(e) => {
            let error = JmapError {
                error_type: "urn:ietf:params:jmap:error:notJSON".to_string(),
                status: Some(400),
                detail: Some(format!("Invalid JSON: {}", e)),
            };
            let json = serde_json::to_string(&error).unwrap();
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(json.into())
                .unwrap());
        }
    };

    // Process method calls
    let mut method_responses = Vec::new();

    for method_call in &jmap_request.method_calls {
        let method_name = method_call.name().unwrap_or("");
        let arguments = method_call.arguments();
        let call_id = method_call.call_id().unwrap_or("0");

        match method_name {
            "Mailbox/get" => {
                let response = handle_mailbox_get(arguments);
                method_responses.push(vec![
                    serde_json::json!("Mailbox/get"),
                    response,
                    serde_json::json!(call_id),
                ]);
            }
            "Email/get" => {
                let response = handle_email_get(arguments);
                method_responses.push(vec![
                    serde_json::json!("Email/get"),
                    response,
                    serde_json::json!(call_id),
                ]);
            }
            "Email/query" => {
                let response = handle_email_query(arguments);
                method_responses.push(vec![
                    serde_json::json!("Email/query"),
                    response,
                    serde_json::json!(call_id),
                ]);
            }
            "Mailbox/query" => {
                let response = handle_mailbox_query(arguments);
                method_responses.push(vec![
                    serde_json::json!("Mailbox/query"),
                    response,
                    serde_json::json!(call_id),
                ]);
            }
            _ => {
                // Unknown method
                let error = serde_json::json!({
                    "type": "urn:ietf:params:jmap:error:unknownMethod",
                    "description": format!("Unknown method: {}", method_name)
                });
                method_responses.push(vec![
                    serde_json::json!("error"),
                    error,
                    serde_json::json!(call_id),
                ]);
            }
        }
    }

    let jmap_response = JmapResponse {
        method_responses,
        created_ids: None,
        session_state: "state-001".to_string(),
    };

    let json = serde_json::to_string(&jmap_response).unwrap();

    Ok(Response::builder()
        .header("Content-Type", "application/json")
        .body(json.into())
        .unwrap())
}

/// Handle Mailbox/get
fn handle_mailbox_get(arguments: Option<&serde_json::Value>) -> serde_json::Value {
    let args: GetRequest = serde_json::from_value(arguments.cloned().unwrap_or_default())
        .unwrap_or(GetRequest {
            account_id: "u1234567890".to_string(),
            ids: None,
            properties: None,
        });

    // Mock mailboxes
    let mailboxes = get_mock_mailboxes();

    let list: Vec<Mailbox> = if let Some(ids) = args.ids {
        mailboxes
            .into_iter()
            .filter(|m| ids.contains(&m.id))
            .collect()
    } else {
        mailboxes
    };

    let response = GetResponse {
        account_id: args.account_id,
        state: "mailbox-state-001".to_string(),
        list,
        not_found: vec![],
    };

    serde_json::to_value(response).unwrap()
}

/// Handle Email/get
fn handle_email_get(arguments: Option<&serde_json::Value>) -> serde_json::Value {
    let args: GetRequest = serde_json::from_value(arguments.cloned().unwrap_or_default())
        .unwrap_or(GetRequest {
            account_id: "u1234567890".to_string(),
            ids: None,
            properties: None,
        });

    // Mock emails
    let emails = get_mock_emails();

    let list: Vec<Email> = if let Some(ids) = args.ids {
        emails.into_iter().filter(|e| ids.contains(&e.id)).collect()
    } else {
        emails
    };

    let response = GetResponse {
        account_id: args.account_id,
        state: "email-state-001".to_string(),
        list,
        not_found: vec![],
    };

    serde_json::to_value(response).unwrap()
}

/// Handle Email/query
fn handle_email_query(_arguments: Option<&serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "accountId": "u1234567890",
        "queryState": "query-state-001",
        "canCalculateChanges": false,
        "position": 0,
        "ids": ["email-001", "email-002"],
        "total": 2,
        "limit": 100
    })
}

/// Handle Mailbox/query
fn handle_mailbox_query(_arguments: Option<&serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "accountId": "u1234567890",
        "queryState": "query-state-001",
        "canCalculateChanges": false,
        "position": 0,
        "ids": ["mailbox-001", "mailbox-002", "mailbox-003"],
        "total": 3,
        "limit": 100
    })
}

/// Get mock mailboxes
fn get_mock_mailboxes() -> Vec<Mailbox> {
    vec![
        Mailbox {
            id: "mailbox-001".to_string(),
            name: "Inbox".to_string(),
            parent_id: None,
            role: Some("inbox".to_string()),
            sort_order: 0,
            total_emails: 42,
            unread_emails: 5,
            total_threads: 38,
            unread_threads: 4,
            my_rights: MailboxRights::default(),
            is_subscribed: true,
        },
        Mailbox {
            id: "mailbox-002".to_string(),
            name: "Sent".to_string(),
            parent_id: None,
            role: Some("sent".to_string()),
            sort_order: 1,
            total_emails: 128,
            unread_emails: 0,
            total_threads: 120,
            unread_threads: 0,
            my_rights: MailboxRights::default(),
            is_subscribed: true,
        },
        Mailbox {
            id: "mailbox-003".to_string(),
            name: "Drafts".to_string(),
            parent_id: None,
            role: Some("drafts".to_string()),
            sort_order: 2,
            total_emails: 3,
            unread_emails: 0,
            total_threads: 3,
            unread_threads: 0,
            my_rights: MailboxRights::default(),
            is_subscribed: true,
        },
    ]
}

/// Get mock emails
fn get_mock_emails() -> Vec<Email> {
    vec![
        Email {
            id: "email-001".to_string(),
            blob_id: "blob-001".to_string(),
            thread_id: "thread-001".to_string(),
            mailbox_ids: {
                let mut map = HashMap::new();
                map.insert("mailbox-001".to_string(), true);
                map
            },
            keywords: {
                let mut map = HashMap::new();
                map.insert("$seen".to_string(), true);
                map
            },
            size: 4521,
            received_at: "2024-01-15T10:30:00Z".to_string(),
            from: Some(vec![EmailAddress {
                name: Some("Alice Smith".to_string()),
                email: "alice@example.com".to_string(),
            }]),
            to: Some(vec![EmailAddress {
                name: Some("Bob Johnson".to_string()),
                email: "bob@example.com".to_string(),
            }]),
            subject: Some("Welcome to JMAP".to_string()),
            preview: Some("This is a preview of the email content...".to_string()),
        },
        Email {
            id: "email-002".to_string(),
            blob_id: "blob-002".to_string(),
            thread_id: "thread-002".to_string(),
            mailbox_ids: {
                let mut map = HashMap::new();
                map.insert("mailbox-001".to_string(), true);
                map
            },
            keywords: HashMap::new(),
            size: 2341,
            received_at: "2024-01-16T14:22:00Z".to_string(),
            from: Some(vec![EmailAddress {
                name: Some("Support Team".to_string()),
                email: "support@example.com".to_string(),
            }]),
            to: Some(vec![EmailAddress {
                name: None,
                email: "user@example.com".to_string(),
            }]),
            subject: Some("Your account is ready".to_string()),
            preview: Some("Thank you for signing up! Your account has been created...".to_string()),
        },
    ]
}

/// Download endpoint
pub async fn download(_req: Request<Body>) -> Result<Response<Body>, wstd::http::Error> {
    Ok(Response::builder()
        .status(StatusCode::NOT_IMPLEMENTED)
        .body("Download not implemented yet\n".into())
        .unwrap())
}

/// Upload endpoint
pub async fn upload(_req: Request<Body>) -> Result<Response<Body>, wstd::http::Error> {
    Ok(Response::builder()
        .status(StatusCode::NOT_IMPLEMENTED)
        .body("Upload not implemented yet\n".into())
        .unwrap())
}

/// 404 handler
pub async fn not_found(_req: Request<Body>) -> Result<Response<Body>, wstd::http::Error> {
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Content-Type", "text/plain")
        .body("Not found\n".into())
        .unwrap())
}
