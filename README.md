![](./assets/rik.png)
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
         

rik is not your typical AI coding assistant. It doesn't do autocomplete or open a chat window. Instead, rik works through markers in your files: it can replace instructions with real content or answer read-only questions in place.

Think of it as leaving sticky notes for an LLM and having someone actually follow through.

## News

### rik 0.3.0 can answer questions

End a marker with `?` and rik answers it without editing the file. Questions can also opt into file-defined dynamic tools.

![rik can answer questions](./assets/rik_answers.png)

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

An instruction can follow the opening delimiter:

```
rik: ( uppercase this
text to transform
)
```

## Examples

### Simple inline replacement

Drop a comment marker above the line you want rewritten.

<table style="width:100%">
<tr><th>Before</th><th>After</th></tr>
<tr><td>

```python
# rik: make it piratey
print("Hello, world!")
```

</td><td>

```python
print("Ahoy, matey!")
```

</td></tr>
</table>

### Multi-line block around existing code

Wrap existing code with a delimited marker to rewrite it.

<table style="width:100%">
<tr><th>Before</th><th>After</th></tr>
<tr><td>

```rust
// rik: [[
// make it recursive
fn factorial(n: u64) -> u64 {
    let mut result = 1;
    for i in 1..=n {
        result *= i;
    }
    result
}
// ]]
```

</td><td>

```rust
fn factorial(n: u64) -> u64 {
    if n <= 1 {
        1
    } else {
        n * factorial(n - 1)
    }
}
```

</td></tr>
</table>

### Complicate it!

rik will faithfully follow even absurd instructions.

<table style="width:100%">
<tr><th>Before</th><th>After</th></tr>
<tr><td>

```ruby
# rik: that's too easy, complicate it
primes = []
limit = 50
(2..limit).each do |candidate|
  is_prime = true
  (2...candidate).each do |divisor|
    if candidate % divisor == 0
      is_prime = false
      break
    end
  end
  primes << candidate if is_prime
end
puts primes.inspect
```

</td><td>

```ruby
π = []
λ = 50
(2..λ).each do |φ|
  ψ = true
  (2...φ).each do |δ|
    if φ % δ == 0
      ψ = false
      break
    end
  end
  π << φ if ψ
end
puts π.inspect
```

</td></tr>
</table>

## Installation

### crates.io

```bash
cargo install rik
```

### Pre-built binaries

Cross-compiled binaries for Linux and macOS (x86_64 and ARM64) are available from the [GitHub Actions / Build](../../actions/workflows/build.yml) workflow runs. Download the artifact archive for your platform from the latest successful run.

### Build from source

```bash
cargo build --release
```

## Configuration

Create `~/.config/rik/rik.toml` with your LLM provider settings and optional diff tool.

```toml
# Optional: custom diff command. Use $pre and $post as placeholders.
diff_tool = ["difft", "--color", "always", "$pre", "$post"]

# Optional: print extra personality around edit tasks.
personality = false

# Keep edits within a single marker's region (recommended default).
marker_limits_edition_range = true

[model]
provider = "openai"
model = "gpt-4o"
# api_key is optional — omit to read from environment variable
# url is optional — omit to use the provider default endpoint
#url = "https://api.openai.com/v1"
```

### Supported providers

| Provider | Config value | Env var | Default URL |
|---|---|---|---|
| OpenAI | `openai` | `OPENAI_API_KEY` | `https://api.openai.com/v1` |
| Anthropic | `anthropic` | `ANTHROPIC_API_KEY` | `https://api.anthropic.com` |
| Gemini | `gemini` | `GEMINI_API_KEY` | `https://generativelanguage.googleapis.com` |
| Ollama | `ollama` | *(none)* | `http://localhost:11434` |
| OpenRouter | `openrouter` | `OPENROUTER_API_KEY` | *(provider default)* |
| xAI | `xai` | `XAI_API_KEY` | *(provider default)* |
| DeepSeek | `deepseek` | `DEEPSEEK_API_KEY` | *(provider default)* |
| Groq | `groq` | `GROQ_API_KEY` | *(provider default)* |
| Together | `together` | `TOGETHER_API_KEY` | *(provider default)* |
| Perplexity | `perplexity` | `PERPLEXITY_API_KEY` | *(provider default)* |
| Mistral | `mistral` | `MISTRAL_API_KEY` | *(provider default)* |
| Cohere | `cohere` | `COHERE_API_KEY` | *(provider default)* |
| Custom endpoint | `openaicompatible` | `OPENAI_API_KEY` | *(required via `url`)* |

The `openaicompatible` provider lets you target any OpenAI-compatible API (LM Studio, vLLM, local proxies, etc.) by setting a custom `url`.

When `diff_tool` is unset, rik auto-detects `difft`, `delta`, or plain `diff`.

## Usage

### Single pass

Scan files matching a glob pattern and complete all markers in one go:

```bash
rik 'src/**/*.rs'
```

Multiple patterns can be joined with commas:

```bash
rik 'src/**/*.rs,tests/**/*.rs'
```

### Context markers

Use slash-delimited markers to provide extra context without content replacement. The marker is removed after processing:

```
rik: /see the type definition above for reference/
```

### Question mode

End a marker with `?` to ask Rik a read-only question:

```
rik: why is this function allocation-heavy?
```

Question markers are handled individually, in top-to-bottom order alongside normal markers. For a question marker, Rik uses a separate read-only prompt with only `read_file` and `list_files`, prints just the answer, and leaves the exact question line untouched. Rik remembers answered question locations in memory so watch mode does not answer the same line repeatedly; restarting Rik clears that memory.

Questions can use dynamic tools defined in their file only when the question contains
`+tool` or `+tools`:

```text
rik: +tool what does the Go documentation say about context cancellation?
```

Additionally, question mark can be put at the beginning or in Rik's callout:

```
rik?: Remind me how to use zls to format arrays in zig
```

Surprised?

```
rik: ??? Why this function returns only a bool
```

### Watch mode

Continuously monitor files and process markers as they appear:

```bash
rik -w 'src/**/*.rs'
```

Press Ctrl+C to stop watching. Press Escape to cancel the current processing loop (Unix only; not supported on Windows).

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

rik gives the agent four file tools during processing:

| Tool | Purpose |
|---|---|
| `read_file` | Read other files for context (types, imports, conventions). Supports offset/limit and omits lines already returned during the current task. |
| `edit_file` | Replace exact text in the target file. Requires unique match and resets `read_file` history after a successful edit. |
| `write_file` | Create new files (refuses to overwrite existing ones). |
| `list_files` | Discover files in the project. Respects `.gitignore`. Supports glob filters. |

All file tools are sandboxed to the current working directory for relative input patterns, or to the absolute directory scope for absolute patterns. The agent can chain these tools across up to 30 turns before producing final edits.

### Dynamic tools

Define command tools directly in a file:

```text
rik +tool (run after editing): zig test src/main.zig
rik +tool: cargo test
rik +tool (read Go documentation): godoc <QUERY>
rik +tool (read files with cat): cat <...>
```

The tool is named `D<N>`, where `N` is the declaration's 1-based line number.
Fixed arguments are passed unchanged, `<NAME>` creates a required lowercase string
parameter, and `<...>` creates a required `args` string array. Commands run directly
from rik's working directory, without a shell.

Dynamic tools are available only while processing the file that defines them.
Normal edit tasks can use them automatically. Questions must explicitly opt in with
`+tool` or `+tools`.

## Guardrails

### Marker stoppers

Add `!rik` or `rik!` within a marker to skip that marker:

```
!rik: leave this task alone
rik: leave this task alone rik!
rik: process this task
```

Stoppers are local to the marker line or multi-line marker block that contains them.
Other markers in the same file are still processed. Use `!{alias}` or `{alias}!`
when using a custom alias.

### Multiple markers

All markers in a single file are processed in one pass. rik won't stop after finding the first one.

## Design philosophy

rik is intentionally limited by design:

- **No REPL** -- you mark up files, run rik, review diffs. Repeat.
- **No arbitrary writes** -- the agent can only edit via `edit_file` which requires exact text matches, and only within the file being processed.
- **No conversation history** -- each invocation is stateless and independent.
- **Diff-first feedback** -- every change produces a diff so you see exactly what was modified.

It's a worker, not a companion. Summon it by name, give it instructions, let it work.

## Rambling

I found gap in LLM-tooling that I couldn't fill otherwise:
- fill-in-middle is very limited when it comes to context - it's fast, but if it can't produce result then it can't and that's it
- agentic development runs amock, by default models try to implement whole feature and it takes more energy to restrict than actually to develop

`rik` is an attempt to fill that gap.

- `rik` is designed to target single file for edition only; most often - single comment (it requires some self-discipline to make multiple ones)
- `rik` can make its own context by listing or reading files

It started as an experiment for agentic tool, but I found `rik` pleasantly ergonomic and decided to release it.

Note: `rig` (the library used for LLM interaction) supports many providers out of the box. If your provider isn't listed above, open an issue or PR.
