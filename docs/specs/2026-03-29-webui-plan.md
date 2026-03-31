# UniClaw Web UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a dashboard-first web UI to UniClaw — embedded in the Rust binary, served by axum, built with Svelte 5.

**Architecture:** Svelte 5 frontend in `web/` directory, built to static files via Vite, embedded in the Rust binary via `rust-embed`. axum serves embedded static files alongside existing API endpoints. New backend endpoints: `/api/chat/stream` (SSE), `/api/config` (GET/POST), `/api/skills` (GET), and `/*` catch-all for SPA.

**Tech Stack:** Rust (axum, rust-embed, SSE), Svelte 5 (runes), Vite, marked.js, custom CSS with CSS variables.

---

## File Map

### New Rust Files
- `src/server/static_files.rs` — serve embedded static files with SPA fallback
- `src/server/api_config.rs` — GET/POST `/api/config` endpoints
- `src/server/api_skills.rs` — GET `/api/skills` endpoint
- `src/server/api_stream.rs` — POST `/api/chat/stream` SSE endpoint

### Modified Rust Files
- `Cargo.toml` — add `rust-embed` dependency
- `src/server/mod.rs` — add new modules
- `src/server/http.rs` — add new routes to router, expand HttpState
- `src/agent/skills.rs` — add public method to get skill metadata as JSON
- `src/config.rs` — add Serialize derive for GET /api/config

### New Frontend Files (all in `web/`)
- `web/package.json`
- `web/vite.config.ts`
- `web/src/app.css` — global styles, CSS variables, dark/light theme
- `web/src/App.svelte` — root: hash router + responsive layout
- `web/src/lib/api.ts` — fetch wrappers for all /api/* endpoints
- `web/src/lib/stream.ts` — SSE client for /api/chat/stream
- `web/src/lib/theme.ts` — dark/light mode state + localStorage persistence
- `web/src/lib/icons.ts` — inline SVG icon components
- `web/src/components/Sidebar.svelte` — desktop sidebar + mobile bottom tabs
- `web/src/components/MetricCard.svelte` — status dashboard card
- `web/src/components/MessageList.svelte` — chat message display
- `web/src/components/ToolCall.svelte` — collapsible tool call indicator
- `web/src/components/Toggle.svelte` — toggle switch
- `web/src/components/Toast.svelte` — notification toast
- `web/src/pages/Dashboard.svelte` — status landing page
- `web/src/pages/Chat.svelte` — chat interface with streaming
- `web/src/pages/Config.svelte` — form-based config editor
- `web/src/pages/Skills.svelte` — skills viewer
- `web/static/manifest.json` — PWA manifest

---

## Task 1: Backend — Add rust-embed and static file serving

**Files:**
- Modify: `Cargo.toml`
- Create: `src/server/static_files.rs`
- Modify: `src/server/mod.rs`
- Modify: `src/server/http.rs`
- Create: `web/dist/index.html` (placeholder)

- [ ] **Step 1: Add rust-embed dependency to Cargo.toml**

Add to `[dependencies]`:
```toml
rust-embed = { version = "8", features = ["interpolate-folder-path"] }
```

- [ ] **Step 2: Create placeholder web/dist/index.html**

```bash
mkdir -p web/dist
```

```html
<!DOCTYPE html>
<html><head><title>UniClaw</title></head>
<body><h1>UniClaw Web UI</h1><p>Placeholder — Svelte build will replace this.</p></body>
</html>
```

- [ ] **Step 3: Create src/server/static_files.rs**

```rust
use axum::{
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web/dist"]
struct Assets;

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try exact file match first
    if let Some(file) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            file.data,
        ).into_response();
    }

    // SPA fallback: serve index.html for all non-API, non-asset routes
    match Assets::get("index.html") {
        Some(file) => Html(std::str::from_utf8(&file.data).unwrap_or("")).into_response(),
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}
```

- [ ] **Step 4: Add mime_guess to Cargo.toml**

```toml
mime_guess = "2"
```

- [ ] **Step 5: Update src/server/mod.rs**

```rust
pub mod http;
pub mod mqtt;
pub mod cron;
pub mod heartbeat;
pub mod static_files;
pub mod api_config;
pub mod api_skills;
pub mod api_stream;
```

- [ ] **Step 6: Update src/server/http.rs — add static fallback route**

Add to the router function, AFTER all `/api/*` routes:

```rust
.fallback(super::static_files::static_handler)
```

- [ ] **Step 7: Build and verify placeholder serves**

```bash
cargo build
OPENAI_API_KEY=test cargo run -- serve
# In another terminal:
curl http://localhost:3000/
# Should return: <h1>UniClaw Web UI</h1>
```

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src/server/ web/dist/index.html
git commit -m "feat: add rust-embed static file serving with SPA fallback"
```

---

## Task 2: Backend — GET /api/config and POST /api/config

**Files:**
- Create: `src/server/api_config.rs`
- Modify: `src/server/http.rs`
- Modify: `src/server/http.rs` (HttpState — add config_path and data_dir)
- Modify: `src/config.rs` (add Serialize to all config structs)
- Modify: `src/main.rs` (pass config_path to HttpState)

- [ ] **Step 1: Add Serialize derive to all Config structs in src/config.rs**

Every struct that has `Deserialize` should also get `Serialize`. The structs that already have both (`AgentConfig`, `LlmConfig`, `ToolsConfig`, `LoggingConfig`, `ServerConfig`, `CronConfig`, `HeartbeatConfig`) are fine. Add `Serialize` to `Config` itself:

```rust
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Config {
```

- [ ] **Step 2: Create src/server/api_config.rs**

```rust
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;

use super::http::HttpState;

pub async fn get_config(
    State(state): State<Arc<HttpState>>,
) -> impl IntoResponse {
    match crate::config::Config::load(&state.config_path) {
        Ok(config) => {
            let mut json = serde_json::to_value(&config).unwrap_or_default();
            // Mask API keys — never send raw keys to frontend
            mask_api_keys(&mut json);
            (StatusCode::OK, Json(json))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to load config: {e}")})),
        ),
    }
}

pub async fn post_config(
    State(state): State<Arc<HttpState>>,
    Json(new_config): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Convert JSON to TOML
    let toml_str = match json_config_to_toml(&new_config) {
        Ok(t) => t,
        Err(e) => return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Invalid config: {e}")})),
        ),
    };

    // Validate by parsing
    if let Err(e) = toml::from_str::<crate::config::Config>(&toml_str) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Config validation failed: {e}")})),
        );
    }

    // Write config file
    if let Err(e) = tokio::fs::write(&state.config_path, &toml_str).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to write config: {e}")})),
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "saved", "message": "Config saved. Restart agent to apply changes."})),
    )
}

fn mask_api_keys(value: &mut serde_json::Value) {
    if let Some(obj) = value.as_object_mut() {
        for (key, val) in obj.iter_mut() {
            if key.contains("api_key") && !key.contains("env") {
                if let Some(s) = val.as_str() {
                    if !s.is_empty() {
                        *val = serde_json::json!("••••••••");
                    }
                }
            }
            mask_api_keys(val);
        }
    }
    if let Some(arr) = value.as_array_mut() {
        for item in arr {
            mask_api_keys(item);
        }
    }
}

fn json_config_to_toml(json: &serde_json::Value) -> Result<String, String> {
    // Parse as our Config type to validate structure, then serialize to TOML
    let config: crate::config::Config = serde_json::from_value(json.clone())
        .map_err(|e| format!("Invalid config structure: {e}"))?;
    toml::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize to TOML: {e}"))
}
```

- [ ] **Step 3: Add config_path and data_dir to HttpState**

In `src/server/http.rs`, add fields:
```rust
pub struct HttpState {
    pub inbound_tx: mpsc::Sender<(Input, oneshot::Sender<Output>)>,
    pub version: String,
    pub model: String,
    pub start_time: std::time::Instant,
    pub config_path: std::path::PathBuf,
    pub data_dir: std::path::PathBuf,
}
```

- [ ] **Step 4: Add routes to router in src/server/http.rs**

```rust
.route("/api/config", get(super::api_config::get_config))
.route("/api/config", post(super::api_config::post_config))
```

- [ ] **Step 5: Update HttpState construction in src/main.rs**

```rust
let http_state = Arc::new(server::http::HttpState {
    inbound_tx: inbound_tx.clone(),
    version: env!("CARGO_PKG_VERSION").into(),
    model: config.llm.model.clone(),
    start_time: std::time::Instant::now(),
    config_path: config_path.clone(),
    data_dir: data_dir.clone(),
});
```

- [ ] **Step 6: Test the endpoints**

```bash
cargo build
OPENAI_API_KEY=test cargo run -- serve &
# GET config:
curl -s http://localhost:3000/api/config | python3 -m json.tool
# Verify API keys are masked
# POST config (read current, save back):
curl -s http://localhost:3000/api/config | curl -s -X POST http://localhost:3000/api/config -H "Content-Type: application/json" -d @-
```

- [ ] **Step 7: Commit**

```bash
git add src/server/api_config.rs src/server/http.rs src/server/mod.rs src/config.rs src/main.rs
git commit -m "feat: add GET/POST /api/config endpoints with API key masking"
```

---

## Task 3: Backend — GET /api/skills

**Files:**
- Create: `src/server/api_skills.rs`
- Modify: `src/server/http.rs` (add route)
- Modify: `src/agent/skills.rs` (add public skills_metadata method)

- [ ] **Step 1: Add skills_metadata method to SkillManager in src/agent/skills.rs**

```rust
impl SkillManager {
    /// Return skill metadata as JSON-serializable structs (for API)
    pub fn skills_metadata(&self) -> Vec<SkillMetadata> {
        self.skills.iter().map(|s| SkillMetadata {
            name: s.name.clone(),
            description: s.description.clone(),
            content: s.content.clone(),
        }).collect()
    }
}

#[derive(serde::Serialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub content: String,
}
```

- [ ] **Step 2: Create src/server/api_skills.rs**

```rust
use axum::{
    extract::State,
    Json,
};
use std::sync::Arc;

use super::http::HttpState;
use crate::agent::skills::SkillManager;

pub async fn get_skills(
    State(state): State<Arc<HttpState>>,
) -> Json<serde_json::Value> {
    let skills_dir = state.data_dir.join("skills");
    let mgr = SkillManager::load(&skills_dir, &[]);
    let metadata = mgr.skills_metadata();
    Json(serde_json::json!({
        "count": metadata.len(),
        "skills": metadata,
    }))
}
```

- [ ] **Step 3: Add route in src/server/http.rs**

```rust
.route("/api/skills", get(super::api_skills::get_skills))
```

- [ ] **Step 4: Test**

```bash
curl -s http://localhost:3000/api/skills | python3 -m json.tool
# Should return: {"count": 3, "skills": [...]}
```

- [ ] **Step 5: Commit**

```bash
git add src/server/api_skills.rs src/server/http.rs src/agent/skills.rs
git commit -m "feat: add GET /api/skills endpoint"
```

---

## Task 4: Backend — POST /api/chat/stream (SSE)

This is the most complex backend task. It adds SSE streaming to the chat endpoint.

**Files:**
- Create: `src/server/api_stream.rs`
- Modify: `src/server/http.rs` (add route)

Note: Full LLM streaming (token-by-token from provider) is a larger change to the LlmProvider trait. For v1, we use a simpler approach: the agent processes the full turn, then we stream the response as SSE events. Tool calls are sent as discrete events. This gives the UI real-time tool call visibility and the final text, without requiring changes to the LLM provider layer.

- [ ] **Step 1: Create src/server/api_stream.rs**

```rust
use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use futures::stream::Stream;
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::oneshot;

use super::http::HttpState;
use crate::agent::{Input, Output};

#[derive(Deserialize)]
pub struct StreamChatRequest {
    message: String,
    #[serde(default = "default_session")]
    session_id: String,
}

fn default_session() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub async fn stream_chat(
    State(state): State<Arc<HttpState>>,
    Json(req): Json<StreamChatRequest>,
) -> impl IntoResponse {
    let input = Input {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: req.session_id.clone(),
        content: req.message,
    };

    let (reply_tx, reply_rx) = oneshot::channel();

    if state.inbound_tx.send((input, reply_tx)).await.is_err() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Agent worker unavailable",
        ));
    }

    let stream = async_stream::stream! {
        // Send thinking status
        yield Ok::<_, Infallible>(Event::default()
            .event("status")
            .data(r#"{"type":"thinking"}"#));

        // Wait for agent response
        match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            reply_rx,
        ).await {
            Ok(Ok(output)) => {
                // Send usage if available
                if let Some(usage) = &output.usage {
                    yield Ok(Event::default()
                        .event("usage")
                        .data(serde_json::json!({
                            "input_tokens": usage.input_tokens,
                            "output_tokens": usage.output_tokens,
                        }).to_string()));
                }

                // Stream the text content in chunks to simulate streaming
                // (true token-by-token streaming requires LlmProvider changes)
                let text = &output.content;
                let chunk_size = 20; // characters per chunk
                let mut pos = 0;
                while pos < text.len() {
                    let end = (pos + chunk_size).min(text.len());
                    // Find a safe UTF-8 boundary
                    let end = if end < text.len() {
                        let mut e = end;
                        while e > pos && !text.is_char_boundary(e) { e -= 1; }
                        if e == pos { end } else { e }
                    } else {
                        end
                    };
                    let chunk = &text[pos..end];
                    yield Ok(Event::default()
                        .event("text_delta")
                        .data(serde_json::json!({"text": chunk}).to_string()));
                    pos = end;
                    // Small delay between chunks for visual streaming effect
                    tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                }

                // Done
                yield Ok(Event::default()
                    .event("done")
                    .data(serde_json::json!({"session_id": req.session_id}).to_string()));
            }
            Ok(Err(_)) => {
                yield Ok(Event::default()
                    .event("error")
                    .data(r#"{"error":"Agent worker dropped request"}"#));
            }
            Err(_) => {
                yield Ok(Event::default()
                    .event("error")
                    .data(r#"{"error":"Request timed out"}"#));
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
```

- [ ] **Step 2: Add async-stream to Cargo.toml**

```toml
async-stream = "0.3"
```

- [ ] **Step 3: Add route in src/server/http.rs**

```rust
.route("/api/chat/stream", post(super::api_stream::stream_chat))
```

- [ ] **Step 4: Test SSE endpoint**

```bash
curl -N -X POST http://localhost:3000/api/chat/stream \
  -H "Content-Type: application/json" \
  -d '{"message": "Hello"}'
# Should see: event: status, event: text_delta (multiple), event: done
```

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/server/api_stream.rs src/server/http.rs src/server/mod.rs
git commit -m "feat: add POST /api/chat/stream SSE endpoint"
```

---

## Task 5: Frontend — Scaffold Svelte 5 project

**Files:**
- Create: `web/package.json`
- Create: `web/vite.config.ts`
- Create: `web/tsconfig.json`
- Create: `web/src/app.css`
- Create: `web/src/App.svelte`
- Create: `web/src/main.ts`
- Create: `web/index.html`
- Create: `web/static/manifest.json`
- Modify: `.gitignore`

- [ ] **Step 1: Create web/package.json**

```json
{
  "name": "uniclaw-web",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite dev",
    "build": "vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "marked": "^15.0.0"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "^5.0.0",
    "svelte": "^5.0.0",
    "typescript": "^5.7.0",
    "vite": "^6.0.0"
  }
}
```

- [ ] **Step 2: Create web/vite.config.ts**

```typescript
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
  server: {
    proxy: {
      '/api': 'http://localhost:3000',
    },
  },
});
```

- [ ] **Step 3: Create web/tsconfig.json**

```json
{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "jsx": "preserve",
    "types": ["svelte"]
  },
  "include": ["src/**/*"]
}
```

- [ ] **Step 4: Create web/index.html**

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <meta name="theme-color" content="#f59e0b" />
  <link rel="manifest" href="/manifest.json" />
  <title>UniClaw</title>
</head>
<body>
  <div id="app"></div>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
```

- [ ] **Step 5: Create web/src/main.ts**

```typescript
import App from './App.svelte';
import './app.css';
import { mount } from 'svelte';

const app = mount(App, { target: document.getElementById('app')! });

export default app;
```

- [ ] **Step 6: Create web/src/app.css — full theme system**

```css
:root {
  --bg: #0a0a0a;
  --surface: #141414;
  --surface-hover: #1c1c1c;
  --border: #262626;
  --text-primary: #e5e5e5;
  --text-secondary: #737373;
  --accent: #f59e0b;
  --accent-hover: #d97706;
  --success: #22c55e;
  --error: #ef4444;
  --radius: 8px;
  --transition: 150ms ease-out;
  --font-body: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  --font-mono: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace;
}

:root[data-theme="light"] {
  --bg: #fafafa;
  --surface: #ffffff;
  --surface-hover: #f5f5f5;
  --border: #e5e5e5;
  --text-primary: #171717;
  --text-secondary: #737373;
}

*, *::before, *::after {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
}

html, body {
  height: 100%;
  font-family: var(--font-body);
  font-size: 15px;
  line-height: 1.5;
  color: var(--text-primary);
  background: var(--bg);
  -webkit-font-smoothing: antialiased;
}

#app {
  height: 100%;
  display: flex;
}

a { color: var(--accent); text-decoration: none; }
a:hover { color: var(--accent-hover); }

code, pre {
  font-family: var(--font-mono);
  font-size: 13px;
}

pre {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 12px 16px;
  overflow-x: auto;
}

button {
  cursor: pointer;
  border: none;
  background: none;
  font: inherit;
  color: inherit;
  transition: all var(--transition);
}

input, textarea, select {
  font: inherit;
  color: var(--text-primary);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 8px 12px;
  transition: border-color var(--transition);
  width: 100%;
}

input:focus, textarea:focus, select:focus {
  outline: none;
  border-color: var(--accent);
}

/* Scrollbar styling */
::-webkit-scrollbar { width: 6px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: var(--border); border-radius: 3px; }
::-webkit-scrollbar-thumb:hover { background: var(--text-secondary); }
```

- [ ] **Step 7: Create web/src/App.svelte — router + layout shell**

```svelte
<script lang="ts">
  import Sidebar from './components/Sidebar.svelte';
  import Dashboard from './pages/Dashboard.svelte';
  import Chat from './pages/Chat.svelte';
  import Config from './pages/Config.svelte';
  import Skills from './pages/Skills.svelte';

  let currentPage = $state(window.location.hash.slice(1) || '/');

  $effect(() => {
    const onHash = () => { currentPage = window.location.hash.slice(1) || '/'; };
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  });

  function navigate(path: string) {
    window.location.hash = path;
  }
</script>

<Sidebar {currentPage} {navigate} />

<main class="main">
  {#if currentPage === '/'}
    <Dashboard />
  {:else if currentPage === '/chat'}
    <Chat />
  {:else if currentPage === '/config'}
    <Config />
  {:else if currentPage === '/skills'}
    <Skills />
  {:else}
    <Dashboard />
  {/if}
</main>

<style>
  .main {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
    max-width: 960px;
  }

  @media (max-width: 768px) {
    .main {
      padding: 16px;
      padding-bottom: 72px; /* space for bottom tabs */
    }
  }
</style>
```

- [ ] **Step 8: Create web/static/manifest.json**

```json
{
  "name": "UniClaw",
  "short_name": "UniClaw",
  "start_url": "/",
  "display": "standalone",
  "theme_color": "#f59e0b",
  "background_color": "#0a0a0a"
}
```

- [ ] **Step 9: Add web/node_modules and web/dist to .gitignore**

Append to `.gitignore`:
```
web/node_modules/
web/dist/
```

- [ ] **Step 10: Install dependencies and verify build**

```bash
cd web && npm install && npm run build && cd ..
# web/dist/ should contain index.html, assets/
cargo build
```

- [ ] **Step 11: Commit**

```bash
git add web/ .gitignore
git commit -m "feat: scaffold Svelte 5 frontend with theme system and hash router"
```

---

## Task 6: Frontend — Sidebar & Icon components

**Files:**
- Create: `web/src/lib/icons.ts`
- Create: `web/src/lib/theme.ts`
- Create: `web/src/components/Sidebar.svelte`

- [ ] **Step 1: Create web/src/lib/icons.ts**

All icons as SVG string constants — 20x20 viewBox, 1.5px stroke, currentColor:

```typescript
export const icons = {
  dashboard: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="7" height="7" rx="1"/><rect x="11" y="2" width="7" height="7" rx="1"/><rect x="2" y="11" width="7" height="7" rx="1"/><rect x="11" y="11" width="7" height="7" rx="1"/></svg>`,
  chat: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4h12a2 2 0 012 2v7a2 2 0 01-2 2H7l-3 3V6a2 2 0 012-2z"/></svg>`,
  config: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><line x1="6" y1="3" x2="6" y2="17"/><line x1="10" y1="3" x2="10" y2="17"/><line x1="14" y1="3" x2="14" y2="17"/><circle cx="6" cy="8" r="2" fill="currentColor"/><circle cx="10" cy="13" r="2" fill="currentColor"/><circle cx="14" cy="6" r="2" fill="currentColor"/></svg>`,
  skills: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6h12M4 10h12M4 14h12"/><rect x="2" y="4" width="16" height="4" rx="1"/><rect x="2" y="8" width="16" height="4" rx="1"/><rect x="2" y="12" width="16" height="4" rx="1"/></svg>`,
  sun: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="10" cy="10" r="4"/><path d="M10 2v2M10 16v2M2 10h2M16 10h2M4.93 4.93l1.41 1.41M13.66 13.66l1.41 1.41M4.93 15.07l1.41-1.41M13.66 6.34l1.41-1.41"/></svg>`,
  moon: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M17 12.5A7.5 7.5 0 117.5 3 5.5 5.5 0 0017 12.5z"/></svg>`,
  chevronRight: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="7 4 13 10 7 16"/></svg>`,
  chevronDown: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 7 10 13 16 7"/></svg>`,
  send: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><line x1="4" y1="10" x2="16" y2="10"/><polyline points="11 5 16 10 11 15"/></svg>`,
  check: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="5 10 8 13 15 6"/></svg>`,
  spinner: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M10 3a7 7 0 016.93 6" class="spin"/></svg>`,
  tool: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="10" cy="10" r="3"/><path d="M10 3v2M10 15v2M3 10h2M15 10h2"/></svg>`,
};
```

- [ ] **Step 2: Create web/src/lib/theme.ts**

```typescript
let theme = $state<'dark' | 'light'>(
  (localStorage.getItem('uniclaw-theme') as 'dark' | 'light') || 'dark'
);

export function getTheme() { return theme; }

export function toggleTheme() {
  theme = theme === 'dark' ? 'light' : 'dark';
  localStorage.setItem('uniclaw-theme', theme);
  document.documentElement.setAttribute('data-theme', theme);
}

// Apply on load
document.documentElement.setAttribute('data-theme', theme);
```

Note: Theme module uses Svelte 5 runes at module level. If that causes issues, wrap in a function that returns a reactive object.

- [ ] **Step 3: Create web/src/components/Sidebar.svelte**

```svelte
<script lang="ts">
  import { icons } from '../lib/icons';
  import { getTheme, toggleTheme } from '../lib/theme';

  let { currentPage, navigate }: { currentPage: string; navigate: (path: string) => void } = $props();

  const navItems = [
    { path: '/', label: 'Status', icon: icons.dashboard },
    { path: '/chat', label: 'Chat', icon: icons.chat },
    { path: '/config', label: 'Config', icon: icons.config },
    { path: '/skills', label: 'Skills', icon: icons.skills },
  ];

  let theme = $derived(getTheme());
</script>

<!-- Desktop sidebar -->
<nav class="sidebar desktop">
  <div class="logo">UC</div>
  {#each navItems as item}
    <button
      class="nav-item"
      class:active={currentPage === item.path}
      onclick={() => navigate(item.path)}
      title={item.label}
    >
      <span class="icon">{@html item.icon}</span>
      <span class="label">{item.label}</span>
    </button>
  {/each}
  <div class="spacer"></div>
  <button class="nav-item" onclick={toggleTheme} title="Toggle theme">
    <span class="icon">{@html theme === 'dark' ? icons.sun : icons.moon}</span>
    <span class="label">Theme</span>
  </button>
</nav>

<!-- Mobile bottom tabs -->
<nav class="tabs mobile">
  {#each navItems as item}
    <button
      class="tab"
      class:active={currentPage === item.path}
      onclick={() => navigate(item.path)}
    >
      <span class="icon">{@html item.icon}</span>
      <span class="label">{item.label}</span>
    </button>
  {/each}
</nav>

<style>
  .sidebar {
    width: 48px;
    background: var(--surface);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 12px 0;
    gap: 4px;
    overflow: hidden;
    transition: width var(--transition);
  }

  .sidebar:hover {
    width: 180px;
  }

  .logo {
    font-size: 16px;
    font-weight: 700;
    color: var(--accent);
    padding: 8px 0 16px;
    white-space: nowrap;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 10px 14px;
    border-radius: 0;
    color: var(--text-secondary);
    white-space: nowrap;
    border-left: 3px solid transparent;
  }

  .nav-item:hover {
    color: var(--text-primary);
    background: var(--surface-hover);
  }

  .nav-item.active {
    color: var(--accent);
    border-left-color: var(--accent);
  }

  .icon { flex-shrink: 0; display: flex; }
  .label { opacity: 0; transition: opacity var(--transition); }
  .sidebar:hover .label { opacity: 1; }

  .spacer { flex: 1; }

  /* Mobile */
  .tabs {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    background: var(--surface);
    border-top: 1px solid var(--border);
    display: flex;
    justify-content: space-around;
    padding: 8px 0;
    padding-bottom: max(8px, env(safe-area-inset-bottom));
    z-index: 100;
  }

  .tab {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 4px 12px;
    color: var(--text-secondary);
    font-size: 11px;
  }

  .tab.active { color: var(--accent); }

  .desktop { display: flex; }
  .mobile { display: none; }

  @media (max-width: 768px) {
    .desktop { display: none; }
    .mobile { display: flex; }
  }
</style>
```

- [ ] **Step 4: Build and verify layout**

```bash
cd web && npm run build && cd ..
cargo run -- serve
# Open http://localhost:3000 — should see sidebar with 4 nav items
```

- [ ] **Step 5: Commit**

```bash
git add web/src/
git commit -m "feat: add sidebar navigation with icons and theme toggle"
```

---

## Task 7: Frontend — Dashboard page

**Files:**
- Create: `web/src/lib/api.ts`
- Create: `web/src/components/MetricCard.svelte`
- Create: `web/src/pages/Dashboard.svelte`

- [ ] **Step 1: Create web/src/lib/api.ts**

```typescript
const BASE = '';

export async function fetchStatus() {
  const res = await fetch(`${BASE}/api/status`);
  return res.json();
}

export async function fetchConfig() {
  const res = await fetch(`${BASE}/api/config`);
  return res.json();
}

export async function saveConfig(config: any) {
  const res = await fetch(`${BASE}/api/config`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(config),
  });
  return res.json();
}

export async function fetchSkills() {
  const res = await fetch(`${BASE}/api/skills`);
  return res.json();
}

export async function sendChat(message: string, sessionId: string) {
  const res = await fetch(`${BASE}/api/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ message, session_id: sessionId }),
  });
  return res.json();
}
```

- [ ] **Step 2: Create web/src/components/MetricCard.svelte**

```svelte
<script lang="ts">
  let { label, value, detail, status }: {
    label: string;
    value: string;
    detail?: string;
    status?: 'ok' | 'warning' | 'error';
  } = $props();
</script>

<div class="card">
  <div class="card-header">
    {#if status}
      <span class="dot dot-{status}"></span>
    {/if}
    <span class="label">{label}</span>
  </div>
  <div class="value">{value}</div>
  {#if detail}
    <div class="detail">{detail}</div>
  {/if}
</div>

<style>
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
    transition: background var(--transition);
  }

  .card:hover {
    background: var(--surface-hover);
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .label {
    font-size: 13px;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .value {
    font-size: 24px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .detail {
    font-size: 13px;
    color: var(--text-secondary);
    margin-top: 4px;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .dot-ok { background: var(--success); }
  .dot-warning { background: var(--accent); }
  .dot-error { background: var(--error); }
</style>
```

- [ ] **Step 3: Create web/src/pages/Dashboard.svelte**

```svelte
<script lang="ts">
  import { fetchStatus } from '../lib/api';
  import MetricCard from '../components/MetricCard.svelte';

  let status = $state<any>(null);
  let error = $state('');

  async function refresh() {
    try {
      status = await fetchStatus();
      error = '';
    } catch (e) {
      error = 'Failed to connect to agent';
    }
  }

  $effect(() => {
    refresh();
    const interval = setInterval(refresh, 5000);
    return () => clearInterval(interval);
  });

  let uptime = $derived(status ? formatUptime(status.uptime_secs) : '--');

  function formatUptime(secs: number): string {
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    return h > 0 ? `${h}h ${m}m` : `${m}m`;
  }
</script>

<div class="page">
  <h1 class="page-title">Status</h1>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <div class="metrics-grid">
    <MetricCard
      label="Agent"
      value={status ? 'Online' : 'Connecting...'}
      detail={uptime}
      status={status ? 'ok' : 'warning'}
    />
    <MetricCard
      label="Model"
      value={status?.model || '--'}
    />
    <MetricCard
      label="Version"
      value={status?.version || '--'}
    />
  </div>
</div>

<style>
  .page {
    max-width: 800px;
  }

  .page-title {
    font-size: 20px;
    font-weight: 600;
    margin-bottom: 24px;
    color: var(--text-primary);
  }

  .metrics-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 12px;
    margin-bottom: 24px;
  }

  .error-banner {
    background: color-mix(in srgb, var(--error) 15%, transparent);
    border: 1px solid var(--error);
    color: var(--error);
    padding: 12px 16px;
    border-radius: var(--radius);
    margin-bottom: 16px;
    font-size: 14px;
  }
</style>
```

- [ ] **Step 4: Build and test dashboard**

```bash
cd web && npm run build && cd ..
cargo run -- serve
# Open http://localhost:3000 — should see Status page with metric cards
```

- [ ] **Step 5: Commit**

```bash
git add web/
git commit -m "feat: add dashboard page with metric cards and auto-refresh"
```

---

## Task 8: Frontend — Chat page with streaming

**Files:**
- Create: `web/src/lib/stream.ts`
- Create: `web/src/components/MessageList.svelte`
- Create: `web/src/components/ToolCall.svelte`
- Create: `web/src/pages/Chat.svelte`

This is the largest frontend task. I'll provide the key files — MessageList, ToolCall, stream.ts, and the Chat page.

- [ ] **Step 1: Create web/src/lib/stream.ts**

```typescript
export interface StreamCallbacks {
  onStatus: (type: string) => void;
  onTextDelta: (text: string) => void;
  onUsage: (usage: { input_tokens: number; output_tokens: number }) => void;
  onDone: (data: any) => void;
  onError: (error: string) => void;
}

export async function streamChat(
  message: string,
  sessionId: string,
  callbacks: StreamCallbacks,
): Promise<void> {
  const response = await fetch('/api/chat/stream', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ message, session_id: sessionId }),
  });

  if (!response.ok) {
    callbacks.onError(`HTTP ${response.status}`);
    return;
  }

  const reader = response.body?.getReader();
  if (!reader) {
    callbacks.onError('No response body');
    return;
  }

  const decoder = new TextDecoder();
  let buffer = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() || '';

    let eventType = '';
    for (const line of lines) {
      if (line.startsWith('event: ')) {
        eventType = line.slice(7).trim();
      } else if (line.startsWith('data: ')) {
        const data = line.slice(6);
        try {
          const parsed = JSON.parse(data);
          switch (eventType) {
            case 'status': callbacks.onStatus(parsed.type); break;
            case 'text_delta': callbacks.onTextDelta(parsed.text); break;
            case 'usage': callbacks.onUsage(parsed); break;
            case 'done': callbacks.onDone(parsed); break;
            case 'error': callbacks.onError(parsed.error); break;
          }
        } catch {}
      }
    }
  }
}
```

- [ ] **Step 2: Create web/src/components/ToolCall.svelte**

```svelte
<script lang="ts">
  import { icons } from '../lib/icons';

  let { tools, totalMs }: {
    tools: Array<{ name: string; status: 'running' | 'done' | 'error'; durationMs?: number }>;
    totalMs?: number;
  } = $props();

  let expanded = $state(false);
</script>

{#if tools.length > 0}
  <button class="tool-summary" onclick={() => expanded = !expanded}>
    <span class="icon">{@html expanded ? icons.chevronDown : icons.chevronRight}</span>
    <span class="text">
      {tools.length} tool{tools.length > 1 ? 's' : ''} used
      {#if totalMs}({totalMs}ms){/if}
    </span>
  </button>

  {#if expanded}
    <div class="tool-details">
      {#each tools as tool}
        <div class="tool-item">
          <span class="tool-icon">
            {#if tool.status === 'running'}
              <span class="spinning">{@html icons.spinner}</span>
            {:else if tool.status === 'done'}
              <span class="done">{@html icons.check}</span>
            {:else}
              <span class="error">!</span>
            {/if}
          </span>
          <span class="tool-name">{tool.name}</span>
          {#if tool.durationMs}
            <span class="tool-duration">{tool.durationMs}ms</span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
{/if}

<style>
  .tool-summary {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 0;
    color: var(--text-secondary);
    font-size: 13px;
  }

  .tool-summary:hover { color: var(--text-primary); }

  .icon { display: flex; }
  .text { flex: 1; }

  .tool-details {
    padding: 4px 0 4px 26px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .tool-item {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .tool-name { font-family: var(--font-mono); }
  .tool-duration { margin-left: auto; }
  .done { color: var(--success); display: flex; }
  .error { color: var(--error); }

  .spinning {
    display: flex;
    animation: spin 1s linear infinite;
    color: var(--accent);
  }

  @keyframes spin { to { transform: rotate(360deg); } }
</style>
```

- [ ] **Step 3: Create web/src/pages/Chat.svelte**

```svelte
<script lang="ts">
  import { streamChat } from '../lib/stream';
  import { marked } from 'marked';
  import ToolCall from '../components/ToolCall.svelte';
  import { icons } from '../lib/icons';

  interface Message {
    role: 'user' | 'assistant';
    content: string;
    timestamp: Date;
    tools?: Array<{ name: string; status: string; durationMs?: number }>;
    totalMs?: number;
  }

  let messages = $state<Message[]>([]);
  let inputText = $state('');
  let isThinking = $state(false);
  let sessionId = $state(localStorage.getItem('uniclaw-session') || 'web');
  let messagesEl: HTMLElement;
  let autoScroll = $state(true);

  function scrollToBottom() {
    if (autoScroll && messagesEl) {
      messagesEl.scrollTop = messagesEl.scrollHeight;
    }
  }

  function onScroll() {
    if (!messagesEl) return;
    const { scrollTop, scrollHeight, clientHeight } = messagesEl;
    autoScroll = scrollHeight - scrollTop - clientHeight < 50;
  }

  async function send() {
    const text = inputText.trim();
    if (!text || isThinking) return;

    inputText = '';
    isThinking = true;
    autoScroll = true;

    messages.push({
      role: 'user',
      content: text,
      timestamp: new Date(),
    });

    let assistantMsg: Message = {
      role: 'assistant',
      content: '',
      timestamp: new Date(),
      tools: [],
    };
    messages.push(assistantMsg);

    const idx = messages.length - 1;

    await streamChat(text, sessionId, {
      onStatus: (type) => {
        // Could show thinking indicator
      },
      onTextDelta: (chunk) => {
        messages[idx].content += chunk;
        requestAnimationFrame(scrollToBottom);
      },
      onUsage: (usage) => {
        // Could display token usage
      },
      onDone: () => {
        isThinking = false;
        requestAnimationFrame(scrollToBottom);
      },
      onError: (error) => {
        messages[idx].content = `Error: ${error}`;
        isThinking = false;
      },
    });
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  function formatTime(date: Date): string {
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }

  function renderMarkdown(text: string): string {
    return marked.parse(text, { breaks: true }) as string;
  }

  $effect(() => {
    localStorage.setItem('uniclaw-session', sessionId);
  });
</script>

<div class="chat-page">
  <div class="chat-header">
    <h1 class="page-title">Chat</h1>
    <span class="session-label">session: {sessionId}</span>
  </div>

  <div class="messages" bind:this={messagesEl} onscroll={onScroll}>
    {#if messages.length === 0}
      <div class="empty">
        <p class="empty-title">Start a conversation</p>
        <p class="empty-detail">Messages are processed by the agent on this device.</p>
      </div>
    {/if}

    {#each messages as msg}
      <div class="message">
        <div class="message-header">
          <span class="message-role" class:user={msg.role === 'user'} class:assistant={msg.role === 'assistant'}>
            {msg.role === 'user' ? 'You' : 'UniClaw'}
          </span>
          <span class="message-time">{formatTime(msg.timestamp)}</span>
        </div>

        {#if msg.tools && msg.tools.length > 0}
          <ToolCall tools={msg.tools} totalMs={msg.totalMs} />
        {/if}

        {#if msg.role === 'assistant'}
          <div class="message-content markdown">{@html renderMarkdown(msg.content)}</div>
        {:else}
          <div class="message-content">{msg.content}</div>
        {/if}
      </div>
    {/each}

    {#if isThinking && messages[messages.length - 1]?.content === ''}
      <div class="thinking">
        <span class="spinning">{@html icons.spinner}</span>
        Thinking...
      </div>
    {/if}
  </div>

  <div class="input-bar">
    <textarea
      class="input"
      bind:value={inputText}
      onkeydown={onKeydown}
      placeholder={isThinking ? 'Thinking...' : 'Message...'}
      disabled={isThinking}
      rows="1"
    ></textarea>
    <button
      class="send-btn"
      class:active={inputText.trim().length > 0}
      onclick={send}
      disabled={isThinking || !inputText.trim()}
    >
      {@html icons.send}
    </button>
  </div>
</div>

<style>
  .chat-page {
    display: flex;
    flex-direction: column;
    height: 100%;
    max-width: 800px;
    margin: 0 auto;
  }

  .chat-header {
    display: flex;
    align-items: baseline;
    gap: 12px;
    margin-bottom: 16px;
    flex-shrink: 0;
  }

  .page-title { font-size: 20px; font-weight: 600; }
  .session-label { font-size: 13px; color: var(--text-secondary); }

  .messages {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding-bottom: 16px;
  }

  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--text-secondary);
  }

  .empty-title { font-size: 16px; color: var(--text-primary); }

  .message-header {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin-bottom: 4px;
  }

  .message-role {
    font-size: 13px;
    font-weight: 600;
  }

  .message-role.user { color: var(--text-secondary); }
  .message-role.assistant { color: var(--accent); }

  .message-time {
    font-size: 12px;
    color: var(--text-secondary);
    margin-left: auto;
  }

  .message-content {
    font-size: 15px;
    line-height: 1.6;
  }

  .message-content :global(pre) {
    margin: 8px 0;
  }

  .message-content :global(code) {
    background: var(--surface);
    padding: 2px 5px;
    border-radius: 4px;
    font-size: 13px;
  }

  .message-content :global(pre code) {
    background: none;
    padding: 0;
  }

  .thinking {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--accent);
    font-size: 14px;
  }

  .spinning {
    display: inline-flex;
    animation: spin 1s linear infinite;
  }

  @keyframes spin { to { transform: rotate(360deg); } }

  .input-bar {
    display: flex;
    gap: 8px;
    padding: 12px 0;
    flex-shrink: 0;
    border-top: 1px solid var(--border);
  }

  .input {
    flex: 1;
    resize: none;
    min-height: 42px;
    max-height: 120px;
  }

  .send-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 42px;
    height: 42px;
    border-radius: var(--radius);
    color: var(--text-secondary);
    background: var(--surface);
    border: 1px solid var(--border);
    flex-shrink: 0;
  }

  .send-btn.active {
    color: var(--bg);
    background: var(--accent);
    border-color: var(--accent);
  }

  .send-btn:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
```

- [ ] **Step 4: Build and test chat**

```bash
cd web && npm run build && cd ..
cargo run -- serve
# Open http://localhost:3000/#/chat
# Type a message and send — should stream response via SSE
```

- [ ] **Step 5: Commit**

```bash
git add web/
git commit -m "feat: add chat page with SSE streaming and tool call display"
```

---

## Task 9: Frontend — Config page

**Files:**
- Create: `web/src/components/Toggle.svelte`
- Create: `web/src/components/Toast.svelte`
- Create: `web/src/pages/Config.svelte`

- [ ] **Step 1: Create web/src/components/Toggle.svelte**

```svelte
<script lang="ts">
  let { checked = false, onchange }: { checked: boolean; onchange: (val: boolean) => void } = $props();
</script>

<button class="toggle" class:on={checked} onclick={() => onchange(!checked)}>
  <span class="knob"></span>
</button>

<style>
  .toggle {
    width: 40px;
    height: 22px;
    border-radius: 11px;
    background: var(--border);
    position: relative;
    flex-shrink: 0;
    transition: background var(--transition);
  }

  .toggle.on { background: var(--accent); }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: white;
    transition: transform var(--transition);
  }

  .toggle.on .knob { transform: translateX(18px); }
</style>
```

- [ ] **Step 2: Create web/src/components/Toast.svelte**

```svelte
<script lang="ts">
  let { message, type = 'info' }: { message: string; type?: 'info' | 'error' | 'success' } = $props();
  let visible = $state(true);

  $effect(() => {
    const timer = setTimeout(() => { visible = false; }, 3000);
    return () => clearTimeout(timer);
  });
</script>

{#if visible}
  <div class="toast toast-{type}">
    {message}
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    top: 16px;
    right: 16px;
    padding: 12px 20px;
    border-radius: var(--radius);
    font-size: 14px;
    z-index: 1000;
    animation: slideIn 200ms ease-out;
  }

  .toast-success {
    background: color-mix(in srgb, var(--success) 15%, var(--surface));
    border: 1px solid var(--success);
    color: var(--success);
  }

  .toast-error {
    background: color-mix(in srgb, var(--error) 15%, var(--surface));
    border: 1px solid var(--error);
    color: var(--error);
  }

  .toast-info {
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text-primary);
  }

  @keyframes slideIn {
    from { opacity: 0; transform: translateX(20px); }
    to { opacity: 1; transform: translateX(0); }
  }
</style>
```

- [ ] **Step 3: Create web/src/pages/Config.svelte**

```svelte
<script lang="ts">
  import { fetchConfig, saveConfig } from '../lib/api';
  import Toggle from '../components/Toggle.svelte';
  import Toast from '../components/Toast.svelte';

  let config = $state<any>(null);
  let original = $state<string>('');
  let saving = $state(false);
  let toast = $state<{ message: string; type: 'success' | 'error' } | null>(null);

  let dirty = $derived(config && JSON.stringify(config) !== original);

  async function load() {
    try {
      config = await fetchConfig();
      original = JSON.stringify(config);
    } catch (e) {
      toast = { message: 'Failed to load config', type: 'error' };
    }
  }

  async function save() {
    if (!config || saving) return;
    saving = true;
    try {
      const result = await saveConfig(config);
      if (result.error) {
        toast = { message: result.error, type: 'error' };
      } else {
        toast = { message: 'Config saved', type: 'success' };
        original = JSON.stringify(config);
      }
    } catch (e) {
      toast = { message: 'Failed to save config', type: 'error' };
    }
    saving = false;
  }

  $effect(() => { load(); });
</script>

{#if toast}
  <Toast message={toast.message} type={toast.type} />
{/if}

<div class="page">
  <h1 class="page-title">Configuration</h1>

  {#if !config}
    <p class="loading">Loading config...</p>
  {:else}
    <section class="section">
      <h2 class="section-title">LLM Provider</h2>
      <div class="field">
        <label>Provider</label>
        <select bind:value={config.llm.provider}>
          <option value="anthropic">Anthropic</option>
          <option value="openai_compatible">OpenAI Compatible</option>
        </select>
      </div>
      <div class="field">
        <label>Model</label>
        <input type="text" bind:value={config.llm.model} />
      </div>
      <div class="field">
        <label>API Key Env Variable</label>
        <input type="text" bind:value={config.llm.api_key_env} />
      </div>
      <div class="field">
        <label>Base URL</label>
        <input type="text" bind:value={config.llm.base_url} />
      </div>
      <div class="field">
        <label>Max Tokens</label>
        <input type="number" bind:value={config.llm.max_tokens} />
      </div>
      <div class="field">
        <label>Temperature ({config.llm.temperature})</label>
        <input type="range" min="0" max="2" step="0.1" bind:value={config.llm.temperature} />
      </div>
    </section>

    <section class="section">
      <h2 class="section-title">Server</h2>
      {#if config.server}
        <div class="field row">
          <label>HTTP Port</label>
          <input type="number" bind:value={config.server.http_port} style="width:100px" />
        </div>
        <div class="field row">
          <label>MQTT</label>
          <Toggle checked={config.server.mqtt_enabled} onchange={(v) => config.server.mqtt_enabled = v} />
        </div>
      {/if}
      {#if config.cron}
        <div class="field row">
          <label>Cron Scheduler</label>
          <Toggle checked={config.cron.enabled} onchange={(v) => config.cron.enabled = v} />
        </div>
      {/if}
      {#if config.heartbeat}
        <div class="field row">
          <label>Heartbeat</label>
          <Toggle checked={config.heartbeat.enabled} onchange={(v) => config.heartbeat.enabled = v} />
        </div>
      {/if}
    </section>

    <section class="section">
      <h2 class="section-title">Tools</h2>
      <div class="field row">
        <label>Shell Commands</label>
        <Toggle checked={config.tools.shell_enabled} onchange={(v) => config.tools.shell_enabled = v} />
      </div>
      <div class="field row">
        <label>HTTP Fetch</label>
        <Toggle checked={config.tools.http_fetch_enabled} onchange={(v) => config.tools.http_fetch_enabled = v} />
      </div>
    </section>

    <div class="save-bar">
      <button class="save-btn" class:dirty disabled={!dirty || saving} onclick={save}>
        {saving ? 'Saving...' : 'Save & Apply'}
      </button>
      {#if dirty}
        <span class="dirty-label">Unsaved changes</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .page { max-width: 600px; }
  .page-title { font-size: 20px; font-weight: 600; margin-bottom: 24px; }
  .loading { color: var(--text-secondary); }

  .section {
    margin-bottom: 32px;
  }

  .section-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 16px;
  }

  .field {
    margin-bottom: 16px;
  }

  .field label {
    display: block;
    font-size: 13px;
    color: var(--text-secondary);
    margin-bottom: 6px;
  }

  .field.row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .field.row label { margin-bottom: 0; }

  .save-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 16px 0;
    border-top: 1px solid var(--border);
    position: sticky;
    bottom: 0;
    background: var(--bg);
  }

  .save-btn {
    padding: 10px 24px;
    border-radius: var(--radius);
    font-weight: 600;
    background: var(--surface);
    color: var(--text-secondary);
    border: 1px solid var(--border);
  }

  .save-btn.dirty {
    background: var(--accent);
    color: var(--bg);
    border-color: var(--accent);
  }

  .save-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .dirty-label {
    font-size: 13px;
    color: var(--accent);
  }
</style>
```

- [ ] **Step 4: Build and test config page**

```bash
cd web && npm run build && cd ..
cargo run -- serve
# Open http://localhost:3000/#/config
# Should load current config, show form fields, save button
```

- [ ] **Step 5: Commit**

```bash
git add web/
git commit -m "feat: add config page with form editing and save"
```

---

## Task 10: Frontend — Skills page

**Files:**
- Create: `web/src/pages/Skills.svelte`

- [ ] **Step 1: Create web/src/pages/Skills.svelte**

```svelte
<script lang="ts">
  import { fetchSkills } from '../lib/api';
  import { marked } from 'marked';
  import { icons } from '../lib/icons';

  let data = $state<any>(null);
  let expandedSkill = $state<string | null>(null);

  $effect(() => {
    fetchSkills().then((d) => { data = d; });
  });

  function toggle(name: string) {
    expandedSkill = expandedSkill === name ? null : name;
  }
</script>

<div class="page">
  <div class="page-header">
    <h1 class="page-title">Skills</h1>
    {#if data}
      <span class="count">{data.count} loaded</span>
    {/if}
  </div>

  {#if !data}
    <p class="loading">Loading skills...</p>
  {:else if data.skills.length === 0}
    <p class="empty">No skills loaded.</p>
  {:else}
    {#each data.skills as skill}
      <button class="skill-card" onclick={() => toggle(skill.name)}>
        <div class="skill-header">
          <div class="skill-info">
            <span class="skill-name">{skill.name}</span>
            <span class="skill-desc">{skill.description}</span>
          </div>
          <span class="chevron">{@html expandedSkill === skill.name ? icons.chevronDown : icons.chevronRight}</span>
        </div>

        {#if expandedSkill === skill.name}
          <div class="skill-content" onclick={(e) => e.stopPropagation()}>
            {@html marked.parse(skill.content)}
          </div>
        {/if}
      </button>
    {/each}
  {/if}

  <p class="hint">Skills are markdown files in data/skills/. Drop a .md file to add a new skill.</p>
</div>

<style>
  .page { max-width: 700px; }
  .page-header { display: flex; align-items: baseline; gap: 12px; margin-bottom: 24px; }
  .page-title { font-size: 20px; font-weight: 600; }
  .count { font-size: 13px; color: var(--text-secondary); }
  .loading, .empty { color: var(--text-secondary); }

  .skill-card {
    display: block;
    width: 100%;
    text-align: left;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
    margin-bottom: 8px;
    transition: background var(--transition);
  }

  .skill-card:hover { background: var(--surface-hover); }

  .skill-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .skill-info { display: flex; flex-direction: column; gap: 4px; }
  .skill-name { font-weight: 600; font-size: 15px; }
  .skill-desc { font-size: 13px; color: var(--text-secondary); }
  .chevron { color: var(--text-secondary); display: flex; }

  .skill-content {
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
    font-size: 14px;
    line-height: 1.6;
  }

  .skill-content :global(ul) { padding-left: 20px; }
  .skill-content :global(code) {
    background: var(--bg);
    padding: 2px 5px;
    border-radius: 4px;
    font-size: 13px;
  }

  .hint {
    margin-top: 24px;
    font-size: 13px;
    color: var(--text-secondary);
    font-style: italic;
  }
</style>
```

- [ ] **Step 2: Build and test skills page**

```bash
cd web && npm run build && cd ..
cargo run -- serve
# Open http://localhost:3000/#/skills
# Should show 3 skills with expandable content
```

- [ ] **Step 3: Commit**

```bash
git add web/
git commit -m "feat: add skills page with expandable markdown content"
```

---

## Task 11: Integration — Final build, test, and polish

**Files:**
- Modify: `web/dist/` (rebuild)
- Modify: `build-release.sh` (add web build step)

- [ ] **Step 1: Add web build step to build-release.sh**

Add before the Rust build loop:

```bash
echo "Building web UI..."
(cd web && npm run build)
echo ""
```

- [ ] **Step 2: Full end-to-end test**

```bash
cd web && npm run build && cd ..
cargo build
OPENAI_API_KEY=your-key cargo run -- serve

# Test in browser:
# 1. http://localhost:3000/         → Dashboard with status cards
# 2. http://localhost:3000/#/chat   → Chat with streaming
# 3. http://localhost:3000/#/config → Config form with save
# 4. http://localhost:3000/#/skills → Skills list
# 5. Test on mobile (phone on same WiFi)
# 6. Test theme toggle (dark → light → dark)
```

- [ ] **Step 3: Build release and verify binary includes web UI**

```bash
./build-release.sh
# Verify: binary serves web UI without separate web/dist/ directory
```

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat: complete web UI — dashboard, chat, config, skills

Svelte 5 frontend embedded in Rust binary via rust-embed.
Pages: Dashboard (status), Chat (SSE streaming), Config (forms), Skills (viewer).
Dark/light theme, mobile responsive, PWA manifest.
Custom amber accent, Inter + JetBrains Mono typography.
~40-60KB gzipped bundle."
```

---

## Summary

| Task | What | New Files |
|------|------|-----------|
| 1 | rust-embed + static serving | static_files.rs |
| 2 | Config API (GET/POST) | api_config.rs |
| 3 | Skills API (GET) | api_skills.rs |
| 4 | SSE streaming endpoint | api_stream.rs |
| 5 | Svelte scaffold + theme | web/* scaffold |
| 6 | Sidebar + icons + theme toggle | Sidebar, icons, theme |
| 7 | Dashboard page | Dashboard, MetricCard, api.ts |
| 8 | Chat page with streaming | Chat, MessageList, ToolCall, stream.ts |
| 9 | Config page with forms | Config, Toggle, Toast |
| 10 | Skills page | Skills |
| 11 | Integration + polish | build script update |

11 tasks, ~30 files, one commit per task.
