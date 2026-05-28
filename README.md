# rik

```
 ______________________________________________________________________
|                                                                      |
|  '########::'####:'##:::'##:                                         |
|   ##.... ##:. ##:: ##::'##::       /\_/\   <- TRAPPED. STOP.         |
|   ##:::: ##:: ##:: ##:'##:::      ( o.o )     IN CODEBASE. STOP.     |
|   ########::: ##:: #####::::       > ^ <      SEND HELP. STOP.       |
|   ##.. ##:::: ##:: ##. ##:::                                         |
|   ##::. ##::: ##:: ##:. ##::                                         |
|   ##:::. ##:'####: ##::. ##:      --= LIMITED AGENT EDITION =--      |
|  ..:::::..::....::..::::..::                                         | 
|____________________________________________________________________  |
|                                                                      |
|  "I literal-ly cannot move unless you write my name in a comment."   |
|                                                                      |
|  [ WARNING: This agent has the spatial awareness of a potted plant ] |
|  [     and will only edit code within radius of its spawn.         ] |
|______________________________________________________________________|
```
         

rik is not your typical AI coding assistant. It doesn't do autocomplete, doesn't chat, and won't try to explain quantum physics. Instead, rik does one thing well: **find markers in your files and replace them with real content.**

Think of it as leaving sticky notes for an LLM and having someone actually follow through.

## How it works

Drop a marker anywhere in a file:

```
rik: add error handling here
```

Run rik against that file (or a glob pattern), and it will read surrounding context, consult other files if needed, and replace the marker line with actual code that fits.

Multi-line instructions are also supported via delimited blocks:

```
rik: [[
Implement a function that parses TOML config from ~/.config/app/config.toml.
Handle missing keys gracefully with sensible defaults.
]]
```

Supported delimiters: `[ ]`, `[[ ]]`, `[[[ ]]]`, `( )`, `(( ))`, `((( )))`, `{ }`, `{{ }}`, `{{{ }}}`.

## Installation

```bash
cargo install --path crates/rik
```

Or build from source:

```bash
cd crates/rik && cargo build --release
```

## Configuration

Create `~/.config/rik/rik.toml` with just two things: where to reach your LLM and how to show diffs.

```toml
[model]
completion_url = "https://api.openai.com/v1"
completion_api_key = "sk-..."
completion_model = "gpt-4o"

# Optional: custom diff command. Use $pre and $post as placeholders.
diff_tool = ["difft", "--color", "always", "$pre", "$post"]
```

When `diff_tool` is unset, rik auto-detects `difft`, `delta`, or plain `diff`.

## Usage

### Single pass

Scan files matching a glob and complete all markers in one go:

```bash
rik 'src/**/*.rs'
```

### Watch mode

Continuously monitor files and process markers as they appear:

```bash
rik -w 'src/**/*.rs'
```

Press Ctrl+C to stop watching.

### Verbose mode

Stream reasoning, tool calls, and text output in real-time:

```bash
rik -v 'src/main.rs'
```

### Custom alias

Use a different trigger word instead of `rik`:

```bash
rik -a todo 'src/**/*.rs'
```

This would look for `todo: <instruction>` markers instead.

## Tools

rik gives the agent three tools during processing:

| Tool | Purpose |
|---|---|
| `read_file` | Read other files for context (types, imports, conventions). Supports offset/limit. |
| `edit_file` | Replace exact text in the target file. Requires unique match. |
| `list_files` | Discover files in the project. Respects `.gitignore`. Supports glob filters. |

The agent can chain these tools across up to 20 turns before producing final edits.

## Guardrails

### Halt marker

Add a guard line to skip processing on a file:

```
!rik
```

If this line exists anywhere in the file, rik skips it entirely even if markers are present. Use `!{alias}` when using a custom alias.

### Multiple markers

All markers in a single file are processed in one pass. rik won't stop after finding the first one.

## Design philosophy

rik is intentionally limited by design:

- **No REPL** -- you mark up files, run rik, review diffs. Repeat.
- **No arbitrary writes** -- the agent can only edit via `edit_file` which requires exact text matches.
- **No conversation history** -- each invocation is stateless and independent.
- **Diff-first feedback** -- every change produces a diff so you see exactly what was modified.

It's a worker, not a companion. Summon it by name, give it instructions, let it work.
