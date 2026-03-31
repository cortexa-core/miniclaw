# UniClaw Web UI — Design Spec

## Overview

A dashboard-first control panel for managing a UniClaw agent running on an edge device. Users primarily chat with the agent via external channels (WhatsApp, Telegram, CLI, HTTP API). The web UI is for monitoring, configuration, and occasional direct chat.

Built with Svelte 5, embedded in the Rust binary via rust-embed, served by the existing axum HTTP server.

## Visual Language

### Color Palette

| Token | Value | Use |
|-------|-------|-----|
| bg | #0a0a0a | Page background |
| surface | #141414 | Cards, panels |
| surface-hover | #1c1c1c | Interactive hover |
| border | #262626 | Card borders, dividers |
| text-primary | #e5e5e5 | Body text, headings |
| text-secondary | #737373 | Labels, timestamps, muted |
| accent | #f59e0b | Amber — primary action, active states, UniClaw identity |
| accent-hover | #d97706 | Amber hover/pressed |
| success | #22c55e | Connected, healthy, complete |
| error | #ef4444 | Disconnected, failed |

Light mode: toggle available. Inverts to #fafafa bg, #171717 text, same amber accent.

### Typography

- Body + headings: Inter (embedded in binary, not CDN) with system font fallback
- Code + tool output: JetBrains Mono or system monospace
- Base size: 15px
- Line height: 1.5 for body, 1.2 for headings

### Components

- Cards: 1px #262626 border, 8px radius, no shadows
- Status dots: 8px circles — green (ok), amber (warning), red (error)
- Toggle switches: custom, amber when on
- Transitions: 150ms ease-out on all interactive elements
- Icons: inline SVGs, 20x20, 1.5px stroke, currentColor, rounded joins. No icon library — ~12 specific icons only.

## Layout & Navigation

### Desktop

Icon-only sidebar (48px) on the left, expands labels on hover (180px). Pages:

| Icon | Label | Route |
|------|-------|-------|
| Grid | Status | / |
| Bubble | Chat | /chat |
| Sliders | Config | /config |
| Layers | Skills | /skills |
| Sun/Moon | Theme toggle | (action, not route) |

Active page: amber left border on sidebar item.

### Mobile

Bottom tab bar with 4 items: Status, Chat, Config, Skills. Active: amber icon + label. Tab bar height: 56px, safe area padding for notched phones.

## Pages

### Dashboard (Status) — Route: /

Landing page. Device health at a glance.

**Tier 1 metric cards (top row, grid):**
- Agent: status dot + uptime
- CPU: temperature
- Memory: used / total

**Tier 2 metric cards (second row):**
- Disk: free space
- Tools: built-in + MCP count
- Skills: loaded count

**Sections below cards:**
- LLM Provider: provider name, model, status dot. Fallback if configured.
- MCP Servers: list with name, transport, tool count, status dot.
- Cron Jobs: count or "No active cron jobs."
- Recent Activity: last 5 agent interactions across all channels. Source (CLI/API/MQTT) + truncated message + relative time.

Data refresh: poll `GET /api/status` every 5 seconds.

### Chat — Route: /chat

For testing and direct interaction.

**Message display:**
- No bubbles — left-aligned text blocks. Name label + timestamp per message.
- "You" in text-secondary, "UniClaw" in amber.
- Timestamps right-aligned, time only (date if different day).
- Markdown rendered in assistant messages (bold, code blocks, lists, links).
- Code blocks: surface background, 1px border, monospace font.

**Tool calls:**
- While running: animated spinner + "Running tool_name..." in amber.
- After complete: collapsed to `▸ N tools used (Xms)` in text-secondary.
- Click to expand: tool name + status checkmark + duration. Click tool to see result.
- Smooth transition between running and collapsed states.

**Input bar:**
- Fixed at bottom of chat area.
- Text input, expands on Shift+Enter for multiline.
- Send button: arrow icon, amber when input has text, muted when empty.
- Enter sends, Shift+Enter newline.
- Disabled with "Thinking..." while agent processing.

**Session:**
- Dropdown in header: `session: cli ▾`
- Lists existing sessions, option for new session.
- Selection persisted in localStorage.

**Streaming:**
- SSE via `POST /api/chat/stream`.
- Text appears incrementally as LLM generates.
- Tool calls appear in real-time (spinner → checkmark).
- Auto-scroll to bottom. Stops if user scrolled up.

### Config — Route: /config

Form-based config editing. No raw TOML.

**Sections (all expanded by default, collapsible):**

1. **LLM Provider**: provider dropdown (Anthropic / OpenAI Compatible), model text, API key (masked, eye toggle), base URL, max tokens, temperature slider, timeout.
2. **Fallback Provider** (optional): same fields, labeled as fallback.
3. **Server**: HTTP port, MQTT toggle + settings, Cron toggle + interval, Heartbeat toggle + interval.
4. **Tools**: Shell toggle + allowed commands + timeout, HTTP Fetch toggle.
5. **MCP Servers**: list with status, delete button. "Add MCP Server" inline form (name, transport, command/url, args).

**Save behavior:**
- `POST /api/config` with full config JSON.
- "Save & Restart" button: amber, disabled until changes detected.
- Footer: "Unsaved changes" (amber) or "Last saved 2m ago" (muted).
- On success: toast notification, agent worker reloads config (re-reads config.toml, rebuilds LLM providers, refreshes context cache). Full process restart not required.
- API key: masked in GET response, only written if user provides new value.

**Validation:**
- Inline — red border + message on invalid fields.
- Port: numeric, 1-65535.
- Temperature: 0.0-2.0 via slider.
- Required fields: provider, model.

### Skills — Route: /skills

View-only list of loaded skills.

**Layout:**
- Page header with count: "3 loaded"
- List of skill cards: name (bold), description (muted), requirements if any.
- Chevron to expand: renders full markdown content of the skill.
- Footer text: "Skills are markdown files in data/skills/. Drop a .md file to add a new skill."

Data from `GET /api/skills`.

## Technical Architecture

### Frontend Stack

- Svelte 5 (runes: $state, $derived, $effect)
- Vite bundler
- Custom CSS with CSS custom properties for theming
- `marked` for markdown rendering (~8KB)
- Hash-based routing (20 lines, no router library)
- No UI framework, no state management library

### Build & Embedding

```
web/src/    →  npm run build  →  web/dist/  →  rust-embed  →  binary
```

`rust-embed` embeds `web/dist/` at compile time. In debug mode, reads from disk (hot reload). In release, everything is in the binary.

### Frontend File Structure

```
web/
  src/
    App.svelte              Router + sidebar layout
    pages/
      Dashboard.svelte
      Chat.svelte
      Config.svelte
      Skills.svelte
    components/
      Sidebar.svelte
      MetricCard.svelte
      MessageList.svelte
      ToolCall.svelte
      ConfigForm.svelte
      SkillCard.svelte
      Toggle.svelte
      Icon.svelte
    lib/
      api.ts                Fetch wrappers for /api/*
      stream.ts             SSE client for /api/chat/stream
      theme.ts              Dark/light mode ($state + localStorage)
  static/
    manifest.json           PWA manifest
  vite.config.ts
  package.json
```

### Backend API (new endpoints needed)

| Method | Path | Purpose |
|--------|------|---------|
| POST | /api/chat/stream | SSE streaming chat (new — requires LLM streaming support) |
| GET | /api/config | Current config as JSON (API keys masked) |
| POST | /api/config | Validate + write config.toml + restart agent |
| GET | /api/skills | List loaded skills with metadata |
| GET | /* | Serve embedded static files (SPA fallback to index.html) |

Existing endpoints unchanged: `POST /api/chat`, `GET /api/status`.

### New Rust Dependencies

- `rust-embed` — embed static files in binary
- SSE via `axum::response::Sse` (already in axum)

### Bundle Size Target

~40-60KB gzipped total (Svelte runtime + app code + CSS + marked + fonts subset).

### PWA

- `manifest.json`: name "UniClaw", theme_color #f59e0b, background_color #0a0a0a
- Installable on phone home screen
- No service worker in v1 (offline caching adds complexity)
