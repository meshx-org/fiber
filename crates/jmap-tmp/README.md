# JMAP Server Implementation

A JMAP (JSON Meta Application Protocol) RFC 8620 compliant server implementation using WASI P2 and wstd.

## Features

- ✅ JMAP Core (RFC 8620)
- ✅ JMAP Mail (RFC 8621) - Basic support
- ✅ Session discovery endpoint (`/.well-known/jmap`)
- ✅ Main API endpoint (`/jmap`)
- ✅ Mock data for testing
- 🚧 Upload/Download endpoints (stubs)
- 🚧 JMAP Submission

## Endpoints

### GET /.well-known/jmap
Returns the JMAP session resource with capabilities, accounts, and URLs.

```bash
curl http://localhost:8080/.well-known/jmap
```

### POST /jmap
Main JMAP API endpoint. Accepts JMAP requests and returns responses.

```bash
curl -X POST http://localhost:8080/jmap \
  -H "Content-Type: application/json" \
  -d '{
    "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
    "methodCalls": [
      ["Mailbox/get", {"accountId": "u1234567890"}, "c1"]
    ]
  }'
```

## Supported Methods

### Mailbox Methods
- `Mailbox/get` - Get mailbox objects
- `Mailbox/query` - Query mailboxes

### Email Methods  
- `Email/get` - Get email objects
- `Email/query` - Query emails

## Mock Data

The server includes mock data:
- 3 mailboxes: Inbox (42 emails), Sent (128 emails), Drafts (3 emails)
- 2 sample emails with full metadata

## Example JMAP Requests

### Get all mailboxes
```json
{
  "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
  "methodCalls": [
    ["Mailbox/get", {"accountId": "u1234567890"}, "c1"]
  ]
}
```

### Get specific emails
```json
{
  "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
  "methodCalls": [
    ["Email/get", {
      "accountId": "u1234567890",
      "ids": ["email-001", "email-002"]
    }, "c1"]
  ]
}
```

### Query and fetch emails
```json
{
  "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
  "methodCalls": [
    ["Email/query", {"accountId": "u1234567890"}, "c1"],
    ["Email/get", {
      "accountId": "u1234567890",
      "#ids": {
        "resultOf": "c1",
        "name": "Email/query",
        "path": "/ids"
      }
    }, "c2"]
  ]
}
```

## Building

```bash
cargo build --target wasm32-unknown-unknown --release -p jmap-tmp
```

## Testing with curl

```bash
# Get session
curl http://localhost:8080/.well-known/jmap | jq

# Get mailboxes
curl -X POST http://localhost:8080/jmap \
  -H "Content-Type: application/json" \
  -d @- <<EOF | jq
{
  "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
  "methodCalls": [
    ["Mailbox/get", {"accountId": "u1234567890"}, "c1"]
  ]
}
EOF
```

## Architecture

```
┌─────────────────────────────────────┐
│  WASI HTTP Handler (wstd)           │
├─────────────────────────────────────┤
│  lib.rs - Route dispatcher          │
├─────────────────────────────────────┤
│  handlers.rs - Request handlers     │
│   - session()                       │
│   - jmap_api()                      │
│   - download() / upload()           │
├─────────────────────────────────────┤
│  types.rs - JMAP type definitions   │
│   - JmapRequest/Response            │
│   - Mailbox, Email, etc.            │
├─────────────────────────────────────┤
│  session.rs - Session resource      │
│   - Capabilities                    │
│   - Accounts                        │
└─────────────────────────────────────┘
```

## RFC Compliance

- **RFC 8620**: JMAP Core - Request/response structure, session resource
- **RFC 8621**: JMAP Mail - Mailbox and Email objects (partial)

## Next Steps

- [ ] Implement Email/set for creating/updating emails
- [ ] Implement Mailbox/set for mailbox management
- [ ] Add persistent storage (WASI KV?)
- [ ] Implement upload/download blob endpoints
- [ ] Add JMAP Submission support
- [ ] Implement Email/changes for efficient sync
- [ ] Add filtering and sorting for queries
- [ ] Authentication and authorization
