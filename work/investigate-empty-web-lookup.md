# Investigate Webpage Lookup Returning Empty

## Issue
When using `rope` to fetch the contents of `https://crates.io/crates/genai`, the output is empty or truncated.

## Potential Causes
1. **Single Page Application (SPA) Rendering**: `crates.io` is a modern frontend application (Ember.js) that renders content dynamically via JavaScript. The simple HTTP `reqwest` client in `tool_lookup.rs` only fetches the raw static HTML, which is just an empty loading container.
2. **User-Agent / Bot Blocking**: The request might be blocked or served a blank page by Cloudflare or crates.io's server because of a generic or missing User-Agent header (currently `Bondage/0.1.0`).

## Proposed Fixes
* **Detect SPA / Empty Results**: If the fetched page contains no meaningful text or only standard SPA boilerplate, warn the user/model.
* **Fallbacks**: Suggest using the crates.io API (e.g., `https://crates.io/api/v1/crates/genai`) or implement basic JSON handling if the target looks like an API.
* **Header Improvements**: Mimic a real browser's `User-Agent` and headers to avoid simple bot-blocking.
