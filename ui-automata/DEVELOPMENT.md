# Development

## Regenerating the workflow JSON Schema

```sh
cargo run -p ui-automata --bin schema-gen
```

Writes `workflow-schema.json` to the workspace root. Add
`# yaml-language-server: $schema=../../workflow-schema.json` at the top of any
`.yml` workflow file to get autocomplete and validation in VS Code (requires the
[YAML extension](https://marketplace.visualstudio.com/items?itemName=redhat.vscode-yaml)).

## Workflow file format

See the field-level docs in [`src/yaml.rs`](src/yaml.rs) — they are compiled
into `workflow-schema.json` and surfaced as tooltips and autocomplete in any
JSON Schema-aware editor or agent.

Key concepts:

- **Anchors** — named, cached handles to live UI elements. Declared once in
  `anchors:`, activated per-phase via `mount:`, and referenced as `scope` in
  every action and condition.
- **Selectors** — CSS-like paths (`>> [role=button][name=Close]`) that locate
  elements within an anchor's subtree.
- **Conditions** — Boolean predicates polled every 100 ms: `ElementFound`,
  `WindowWithAttribute`, `DialogPresent`, `AnyOf`, `Not`, etc.
- **Steps** — action + expect condition pairs. Optional `precondition` skips the
  step (not an error) when false.
- **Phases** — ordered groups of steps with `mount`/`unmount` lifecycle and
  optional `precondition` (false = skip phase, not an error).
- **Recovery handlers** — fire when a step times out in a known bad state;
  run corrective actions then `retry_step`, `skip_step`, or `fail`.

## Running tests

```sh
# Unit + mock tests (Windows not required)
cargo test -p ui-automata
```

## Linting all workflows

```sh
cargo run --bin ui-workflow-check -- $(find workflows -name '*.yml' | tr '\n' ' ')
```