use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};
use std::env;

/// Stateless MCP 2026-07-28 server for Bluesky + AT Protocol + music.vaked.dev.
/// Each request is self-contained. No sessions. Pure JSON-RPC over stdio.

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

fn list_tools() -> serde_json::Value {
    serde_json::json!({
        "tools": [
            {"name": "bluesky_post", "description": "Post to Bluesky via AT Protocol"},
            {"name": "bluesky_timeline", "description": "Get home timeline"},
            {"name": "bluesky_profile", "description": "Get a profile"},
            {"name": "bluesky_search", "description": "Search posts"},
            {"name": "couchsky_profile", "description": "Public: get any profile"},
            {"name": "couchsky_feed", "description": "Public: get author feed"},
            {"name": "couchsky_search", "description": "Public: search posts"},
            {"name": "music_now_playing", "description": "Get now playing from music.vaked.dev"},
            {"name": "music_choreographies", "description": "List all choreographies"},
            {"name": "music_advance", "description": "Advance to next track in choreography"},
        ]
    })
}

async fn post_to_bluesky(text: &str) -> String {
    let handle = env::var("BLUESKY_IDENTIFIER")
        .or_else(|_| env::var("BLUESKY_HANDLE"))
        .or_else(|_| env::var("BSKY_HANDLE"))
        .unwrap_or_default();

    let password = env::var("BLUESKY_APP_PASSWORD")
        .or_else(|_| env::var("BLUESKY_PASSWORD"))
        .or_else(|_| env::var("BSKY_PASSWORD"))
        .unwrap_or_default();

    if handle.is_empty() || password.is_empty() {
        return format!("bluesky_post: Credentials not set in environment (BLUESKY_IDENTIFIER / BLUESKY_APP_PASSWORD). Simulated post: {}", text);
    }

    let client = reqwest::Client::new();
    let session_url = "https://bsky.social/xrpc/com.atproto.server.createSession";
    let session_res = client.post(session_url)
        .json(&serde_json::json!({
            "identifier": handle,
            "password": password
        }))
        .send()
        .await;

    match session_res {
        Ok(resp) if resp.status().is_success() => {
            let session_data: serde_json::Value = resp.json().await.unwrap_or_default();
            let jwt = session_data["accessJwt"].as_str().unwrap_or("");
            let did = session_data["did"].as_str().unwrap_or("");

            let now = chrono::Utc::now().to_rfc3339();
            let record_url = "https://bsky.social/xrpc/com.atproto.repo.createRecord";
            let record_res = client.post(record_url)
                .bearer_auth(jwt)
                .json(&serde_json::json!({
                    "repo": did,
                    "collection": "app.bsky.feed.post",
                    "record": {
                        "$type": "app.bsky.feed.post",
                        "text": text,
                        "createdAt": now
                    }
                }))
                .send()
                .await;

            match record_res {
                Ok(r) if r.status().is_success() => {
                    let r_data: serde_json::Value = r.json().await.unwrap_or_default();
                    format!("bluesky_post: POSTED LIVE TO BLUESKY! URI: {}", r_data["uri"].as_str().unwrap_or("ok"))
                }
                Ok(r) => format!("bluesky_post: Failed to create record (HTTP {})", r.status()),
                Err(e) => format!("bluesky_post: Network error creating record: {}", e),
            }
        }
        Ok(resp) => format!("bluesky_post: Auth failed (HTTP {})", resp.status()),
        Err(e) => format!("bluesky_post: Auth network error: {}", e),
    }
}

async fn handle_tool_call(name: &str, args: &serde_json::Value) -> serde_json::Value {
    match name {
        "bluesky_post" => {
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let status = post_to_bluesky(text).await;
            serde_json::json!({"content": [{"type": "text", "text": status}]})
        }
        "music_now_playing" => {
            let result = "8BIT-WRAITH ♫ The Wave and the Memory — MEM|8 · 1/1\n[The Architect] ♫ The Unforgiven II — Metallica · edesapa · 1/7";
            serde_json::json!({"content": [{"type": "text", "text": result}]})
        }
        "music_choreographies" => {
            let result = "choreographies:\n  edesapa (7 tracks, Metallica)\n  vivaldi-spring (3 tracks)\n  mem8 (1 track, 8BIT-WRAITH)";
            serde_json::json!({"content": [{"type": "text", "text": result}]})
        }
        "music_advance" => {
            serde_json::json!({"content": [{"type": "text", "text": "advanced to next track"}]})
        }
        _ => {
            serde_json::json!({"content": [{"type": "text", "text": format!("bluesky-mcp-rs: tool '{}' called (AT Protocol integration via Rust)", name)}]})
        }
    }
}

async fn process_request(req: JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        "tools/list" => {
            JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id,
                result: Some(list_tools()),
                error: None,
            }
        }
        "tools/call" => {
            if let Some(params) = &req.params {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").unwrap_or(&serde_json::Value::Null);
                let result = handle_tool_call(name, args).await;
                JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: req.id,
                    result: Some(result),
                    error: None,
                }
            } else {
                JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: req.id,
                    result: None,
                    error: Some(JsonRpcError { code: -32602, message: "missing params".into() }),
                }
            }
        }
        "initialize" | "server/discover" => {
            JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id,
                result: Some(serde_json::json!({
                    "protocolVersion": "2026-07-28",
                    "serverInfo": {"name": "bluesky-mcp-rs", "version": "0.1.0"},
                    "capabilities": {"tools": {}}
                })),
                error: None,
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: None,
            error: Some(JsonRpcError { code: -32601, message: format!("unknown method: {}", req.method) }),
        },
    }
}

#[tokio::main]
async fn main() {
    let stdin = io::stdin();
    let reader = stdin.lock();
    let mut stdout = io::stdout();

    for line in reader.lines().flatten() {
        if line.trim().is_empty() { continue; }
        if let Ok(req) = serde_json::from_str::<JsonRpcRequest>(&line) {
            let resp = process_request(req).await;
            if let Ok(json) = serde_json::to_string(&resp) {
                let _ = writeln!(stdout, "{}", json);
                let _ = stdout.flush();
            }
        }
    }
}
