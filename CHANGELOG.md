# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **`marker_limits_edition_range` config** — when enabled (default), `edit_file` rejects edits whose text spans across multiple markers, ensuring each edit stays scoped to a single marker region

## [0.2.1] - 2026-05-30

### Added
- **`write_file` tool** — agents can now create new files (refuses to overwrite existing ones)
- Integration tests for `edit_file` with multi-byte (emoji) content

### Changed
- Personality adjustments
- Better `write_file` output formatting

### Fixed
- Fix path traversal vulnerability in `validate_relative_path` for non-existent paths
- Fix: guarantee all markers are removed after agent completes, not just context markers
- Make keyboard Space-bar polling Unix-only (`termios` unavailable on Windows); no-op on non-Unix platforms

## [0.2.0] - 2026-05-30

### Added

- **Context markers** — `rik: /slash-delimited/` markers provide extra context to the agent and are auto-removed after completion (no content replacement)
- **Marker span auto-update** — line positions are recalculated after each edit, keeping multi-marker files consistent
- **File sandboxing** — `read_file`, `write_file`, and `list_files` tools are restricted to the current working directory and use relative paths in output
- **Edit tool path scoping** — `edit_file` can only edit the file currently being processed; `file_path` argument removed from tool schema
- **Marker line-range enforcement** — edits near a marker are validated against Prolog-style endpoint logic (Q/P matching)
- **Personality module** — replaces standalone `MoodifyTool` with a `Mood` enum + `moodify` function, `pre_work`/`post_work` quotes, and MOTD display
- **Keyboard listener** — press Space during watch mode to stop the current processing loop
- **RAII file reverter** — `FileReverter` guard automatically reverts partial edits on early return or cancellation; integrated with `Drop` and Ctrl+C cleanup
- **Watch mode deduplication** — tracks file content hashes to skip unchanged files, eliminating duplicate processing
- **Nested bracket balancing** — closing delimiters no longer require alias prefix; bracket depth is tracked atomically

### Changed

- Improved tool-call logging: human-readable argument formatting for `list_files`, `read_file`, and `edit_file` instead of raw JSON
- Personality quote printing no longer uses a random delay (immediate output)
- `complete_marker.rs` moved from `tools/` to `src/markers.rs` as a top-level module
- Removed unused `CompleteMarkerTool` from the tool registry

### Fixed

- `edit_near_marker` now correctly checks edit endpoints against each marker line rather than doing range-overlap detection
- `div_ceil` padding in personality box replaced with idiomatic Rust
- Multiple clippy warnings resolved (unused imports, unnecessary casts, collapsed else-if chains, `.contains()` vs `.iter().any()`)

## [0.1.1] - 2026-05-28

### Added
- Swappable LLM provider support via config — 13 providers: OpenAI, Anthropic, Gemini, Ollama, OpenRouter, xAI, DeepSeek, Groq, Together, Perplexity, Mistral, Cohere, and OpenAI-compatible custom endpoints
- Provider-aware API key resolution: explicit config value > environment variable per provider
- Support for comma-separated glob patterns (e.g. `rik 'src/**/*.rs,tests/**/*.rs'`)
- CI workflow with fmt/clippy/test checks and cross-compiled builds (Linux x86_64/ARM64, macOS x86_64/ARM64, Windows MSVC)
- `cargo install rik` installation method documented in README
- Pre-built cross-compiled binaries available from GitHub Actions

### Changed
- Config format rewritten: replaced `completion_url`/`completion_api_key`/`completion_model` with `provider`, `url`, `api_key`, `model` (**breaking config change**)
- Completion engine made generic over `CompletionClient` instead of hardcoded to OpenAI
- Updated installation instructions in README (no longer a workspace sub-crate)

### Fixed
- Verbose reasoning output: reasoning tag reset now only fires when verbose mode is active, preventing stray ANSI escape sequences

## [0.1.0] - 2026-05-28

### Added

- Core marker completion engine: scan files for `{alias}: <query>` markers and replace with LLM-generated content
- Support for multi-line delimited markers (`[[...]]`, `((...))`, `{{...}}`, etc.)
- CLI with single-pass and watch mode (`-w`) via `notify` crate
- Configurable diff tool auto-detection (`difft` > `delta` > `diff`)
- TOML-based configuration at `~/.config/rik/rik.toml`
- Custom alias support via `-a` flag
- Verbose mode (`-v`) for streaming agent reasoning and tool calls
- Halt marker (`!{alias}`) to skip processing on a file
- Rig AI tools exposed to the LLM agent:
  - `read_file` — read file contents with optional offset/limit
  - `write_file` — create new files (refuses overwrites)
  - `edit_file` — exact-text replacement with uniqueness enforcement
  - `list_files` — directory listing respecting `.gitignore`
  - `complete_marker` — targeted marker replacement
- Comprehensive test suite covering marker parsing, file operations, and edge cases
