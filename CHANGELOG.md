# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
