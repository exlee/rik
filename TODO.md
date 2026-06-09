# TODO

## Reliability

- Fix stream-error behavior so incomplete runs roll back instead of silently
  deleting unresolved markers.
- Extend rollback to files created through `write_file`.
- Define and document the intended `send_message` lifecycle.
- Add orchestration tests for cancellation, stream failure, created files, and
  marker cleanup.

## Maintenance

- Repair README drift:
  - Correct the documented tool count and include `send_message`.
  - Update the documented maximum tool turns from 20 to 30.
  - Reconcile the design-philosophy text with `write_file`.
  - Document `personality` and `marker_limits_edition_range`.
- Fix the current `cargo fmt --all --check` and
  `cargo clippy --all-targets --all-features -- -D warnings` failures.

## Possible Addition

- Consider a passive post-edit command hook, for example:

  ```toml
  post_edit = ["cargo", "test"]
  ```

  Run it after edits and print its status and output. Do not feed the result
  back to the model, retry automatically, or widen the editing scope.

## Ideas

- Print out which lines `read_file` is reading - otherwise it looks like it's reading same file over and over
- `read_file` / list_files should show path relatie to watched dir (shorter output)
- `edit_file` could maybe show diff or at least lines  edited, otherwise output is useless
- (maybe) `read_file` shouldn't allow rik to bloat context especially in consequent reads
- Change `Stopped` -> `User cancel` and consider changing `space` to `esc` instead

## Out Of Scope

- Comments-only marker recognition. Rik is intentionally format-agnostic and
  can be used in Markdown, TOML, text files, prose, and code. `--alias` is the
  escape hatch for collisions.
- JSON output. External harnesses can provide it when needed.
- Configurable edit radius or per-marker policies. The fixed seven-line radius
  is an intentional internal constraint.
- Dry-run mode. Existing editor undo support, constrained edits, and diff
  output already cover the practical workflow.
