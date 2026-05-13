# psy-wasm

WASM bindings for compiling PSY source code in-memory.

## Exports

- `init_logging()`
- `compile_source(source: string): string`
- `compile_project(filesJson: string): string`

## `compile_project` input

`filesJson` must use the explicit project format:

```json
{
  "entry": ["main"],
  "method_names": ["main"],
  "files": [
    [["main"], "mod foo;\nfn main() { foo::run(); }"],
    [["foo"], "pub fn run() {}"]
  ]
}
```

`entry` is required.
If the entry file is not `main.psy`, you must provide `method_names`.

The return value is a JSON string with:

- `success`
- `error`
- `error_offset`
- `entry_path`
- `compile_results`
- `abi`

## Node smoke test

Build the Node-target package:

```bash
wasm-pack build psy-wasm --target nodejs --dev --out-dir pkg-node
```

Run the included demo:

```bash
cd psy-wasm/demo-node
npm test
```

It validates:

- single-file compile success
- multi-file in-memory project compile success
- parse failure returns `error_offset`

## Browser demo

Build the web-target package:

```bash
wasm-pack build psy-wasm --target web --dev --out-dir pkg-web
```

Start a static server from the repo root:

```bash
python3 -m http.server 4173
```

Open:

```text
http://127.0.0.1:4173/psy-wasm/demo-web/
```

The page includes:

- an editable source panel
- example snippets seeded from `tests/`
- a `Compile Source` button
- a `Compile Module Project` button for the multi-file sample
