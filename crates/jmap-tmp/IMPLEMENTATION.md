# JMAP Implementation Guide

## Overview

This is a JMAP (RFC 8620/8621) compliant server built with:
- **WASI P2** (Preview 2) - WebAssembly System Interface
- **wstd** - WebAssembly standard library for HTTP handling
- **Rust + serde** - Type-safe JSON serialization

## Project Structure

```
crates/jmap-tmp/
├── src/
│   ├── lib.rs          # Main entry point, routing
│   ├── session.rs      # JMAP session resource
│   ├── types.rs        # JMAP type definitions
│   └── handlers.rs     # Request handlers and mock data
├── wit/
│   └── world.wit       # WebAssembly interface
├── Cargo.toml
├── README.md
├── IMPLEMENTATION.md   # This file
└── test-requests.sh    # Test script
```

## How It Works

### 1. Entry Point (`lib.rs`)

```rust
#[wstd::http_server]
async fn main(req: Request<Body>) -> Result<Response<Body>, wstd::http::Error>
```

The `#[wstd::http_server]` macro makes this function a WASI HTTP handler that:
- Receives HTTP requests via WASI P2
- Routes to appropriate handlers
- Returns HTTP responses

### 2. Routing

Requests are routed based on method and path:

- `GET /.well-known/jmap` → Session discovery (RFC 8620 §2)
- `POST /jmap` → Main API endpoint (RFC 8620 §3)
- `GET /download/*` → Download blobs (stub)
- `POST /upload/*` → Upload blobs (stub)

### 3. JMAP Request/Response Flow

```
Client Request → Parse JSON → Process Method Calls → Build Response → Send JSON
```

#### Request Structure (RFC 8620 §3.3)
```json
{
  "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
  "methodCalls": [
    ["Mailbox/get", {"accountId": "u123"}, "call-id-1"]
  ]
}
```

#### Response Structure (RFC 8620 §3.4)
```json
{
  "methodResponses": [
    ["Mailbox/get", {"accountId": "u123", "state": "...", "list": [...]}, "call-id-1"]
  ],
  "sessionState": "state-001"
}
```

### 4. Method Processing

Each method call is processed sequentially:

1. **Extract** method name, arguments, and call ID
2. **Dispatch** to appropriate handler
3. **Build** response with same call ID
4. **Collect** all responses into method_responses array

### 5. Type Safety with Serde

All JMAP types are strongly typed:

```rust
#[derive(Serialize, Deserialize, Debug)]
pub struct JmapRequest {
    pub using: Vec<String>,
    #[serde(rename = "methodCalls")]
    pub method_calls: Vec<MethodCall>,
}
```

Serde handles:
- JSON ↔ Rust struct conversion
- Field renaming (`methodCalls` → `method_calls`)
- Optional fields
- Validation

## Supported JMAP Methods

### Core Methods

#### Mailbox/get
Get mailbox objects by ID or all mailboxes.

**Arguments:**
```json
{
  "accountId": "u1234567890",
  "ids": ["mailbox-001"],  // Optional
  "properties": null        // Optional
}
```

**Returns:** `GetResponse<Mailbox>`

#### Mailbox/query
Query mailboxes (returns IDs).

**Returns:** Query response with mailbox IDs

#### Email/get
Get email objects by ID.

**Arguments:**
```json
{
  "accountId": "u1234567890",
  "ids": ["email-001", "email-002"],
  "properties": null
}
```

**Returns:** `GetResponse<Email>`

#### Email/query  
Query emails (returns IDs).

**Returns:** Query response with email IDs

## Mock Data

The implementation includes mock data for testing:

### Mailboxes
- **Inbox** (mailbox-001): 42 emails, 5 unread
- **Sent** (mailbox-002): 128 emails, 0 unread
- **Drafts** (mailbox-003): 3 emails, 0 unread

### Emails
- **email-001**: From Alice, "Welcome to JMAP"
- **email-002**: From Support, "Your account is ready"

## Adding New Methods

To add a new JMAP method:

### 1. Add handler in `handlers.rs`:

```rust
fn handle_mailbox_set(arguments: Option<&serde_json::Value>) -> serde_json::Value {
    // Parse arguments
    // Perform operation
    // Return response
    serde_json::json!({
        "accountId": "u1234567890",
        "oldState": "state-001",
        "newState": "state-002",
        "created": {},
        "updated": {},
        "destroyed": []
    })
}
```

### 2. Add to dispatcher in `jmap_api()`:

```rust
match method_name {
    "Mailbox/set" => {
        let response = handle_mailbox_set(arguments);
        method_responses.push(vec![
            serde_json::json!("Mailbox/set"),
            response,
            serde_json::json!(call_id),
        ]);
    }
    // ... other methods
}
```

### 3. Define types if needed in `types.rs`:

```rust
#[derive(Serialize, Deserialize, Debug)]
pub struct SetRequest<T> {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create: Option<HashMap<String, T>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update: Option<HashMap<String, T>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destroy: Option<Vec<String>>,
}
```

## Error Handling

JMAP errors follow RFC 8620 §3.6.1:

```rust
let error = JmapError {
    error_type: "urn:ietf:params:jmap:error:unknownMethod".to_string(),
    status: Some(400),
    detail: Some("Method not found".to_string()),
};
```

Common error types:
- `urn:ietf:params:jmap:error:notJSON` - Invalid JSON
- `urn:ietf:params:jmap:error:unknownMethod` - Unknown method
- `urn:ietf:params:jmap:error:invalidArguments` - Invalid args
- `urn:ietf:params:jmap:error:accountNotFound` - Account not found

## WASI P2 Integration

The component exports WASI HTTP handler interface:

```wit
world jmap-tmp {
   export wasi:http/incoming-handler@0.2.4;
}
```

This allows:
- Deployment to any WASI P2 runtime
- Composition with other WASM components
- Sandboxed execution
- Language-agnostic integration

## Testing

### 1. Build the component:
```bash
cargo build --target wasm32-unknown-unknown --release -p jmap-tmp
```

### 2. Run with a WASI runtime:
```bash
# Example with wasmtime
wasmtime serve ./target/wasm32-unknown-unknown/release/jmap_tmp.wasm
```

### 3. Test endpoints:
```bash
./test-requests.sh
```

## Future Enhancements

### Short Term
- [ ] Implement `/set` methods (create, update, delete)
- [ ] Add proper state management for sync
- [ ] Implement result references (`#ids`)
- [ ] Add filtering in query methods

### Medium Term
- [ ] Persistent storage with WASI KV
- [ ] Upload/download blob handling
- [ ] JMAP Submission (RFC 8621)
- [ ] Email search and filtering
- [ ] Push notifications (EventSource)

### Long Term
- [ ] Authentication (OAuth 2.0)
- [ ] Multi-tenancy
- [ ] JMAP Contacts (RFC 8605)
- [ ] JMAP Calendars (RFC 8607)
- [ ] Performance optimization
- [ ] Comprehensive test suite

## References

- [RFC 8620: JMAP Core](https://www.rfc-editor.org/rfc/rfc8620.html)
- [RFC 8621: JMAP Mail](https://www.rfc-editor.org/rfc/rfc8621.html)
- [WASI Preview 2](https://github.com/WebAssembly/WASI/tree/main/preview2)
- [Component Model](https://github.com/WebAssembly/component-model)

## Contributing

When adding features, ensure:
1. **RFC compliance** - Follow JMAP specifications
2. **Type safety** - Use Rust types, avoid stringly-typed code
3. **Error handling** - Return proper JMAP errors
4. **Testing** - Add test cases to `test-requests.sh`
5. **Documentation** - Update README and this guide
