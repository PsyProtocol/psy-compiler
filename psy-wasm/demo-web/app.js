import init, { compile_dargo_project, compile_project, compile_source, init_logging } from "../pkg-web/psy_wasm.js";

const EXAMPLES = {
  std: `// Explicit std import in wasm demo
use std::prelude::*;

// input: 2,3
// output: 5
fn main(a: Felt, b: Felt) -> Felt {
    assert(a < b, "a should be smaller than b");
    assert_eq(a + b, 5, "sum should be 5");
    a + b
}`,
  assert: `// From tests/assert_test.psy
// input: 2,3
// output: 5
// team: psy
fn main(a: Felt, b: Felt) -> Felt {
    assert(a < b, "a != b");
    assert_eq(b - a, 1, "b - a != 1");
    return a + b;
}`,
  array: `// From tests/array_test.psy
struct HW {
    pub height: Felt,
    pub weight: Felt,
}

struct Person {
    pub age: Felt,
    pub hw: [HW; 2],
}

// input:0,1
// output: 340
fn main(a: Felt, b: Felt) -> Felt {
    let hw1: HW = new HW {
        height: 180,
        weight: 140,
    };
    let hw2: HW = new HW {
        height: 175,
        weight: 110,
    };
    let person1: Person = new Person {
        age: 8,
        hw: [hw1, hw1],
    };
    let person2: Person = new Person {
        age: 18,
        hw: [hw2, hw2],
    };
    let mut arr: [Person; 2] = [person1, person2];
    arr[a].hw[b] = new HW {
        height: 160,
        weight: 110,
    };
    return arr[a].hw[0].height + arr[a].hw[1].height;
}`,
  module: `mod foo;

fn main() {
    foo::run();
}`,
};

const editor = document.querySelector("#source-editor");
const wasmStatus = document.querySelector("#wasm-status");
const compileSummary = document.querySelector("#compile-summary");
const resultBanner = document.querySelector("#result-banner");
const compileOutput = document.querySelector("#compile-output");
const abiOutput = document.querySelector("#abi-output");
const compileSourceButton = document.querySelector("#compile-source-button");
const compileProjectButton = document.querySelector("#compile-project-button");
const compileDargoButton = document.querySelector("#compile-dargo-button");
const exampleButtons = document.querySelectorAll(".example-button");
const params = new URLSearchParams(window.location.search);

let wasmReady = false;
let activeExample = "assert";

function setBanner(kind, text) {
  resultBanner.className = `result-banner ${kind}`;
  resultBanner.textContent = text;
}

function setBusy(isBusy) {
  compileSourceButton.disabled = isBusy;
  compileProjectButton.disabled = isBusy;
  compileDargoButton.disabled = isBusy;
}

function renderResult(result) {
  compileOutput.textContent = JSON.stringify(result, null, 2);
  abiOutput.textContent = result.abi ? JSON.stringify(result.abi, null, 2) : "No ABI extracted.";

  if (result.success) {
    compileSummary.textContent = "Compile succeeded";
    setBanner("success", `Compile succeeded for ${result.entry_path ?? "virtual entry"}.`);
  } else {
    compileSummary.textContent = "Compile failed";
    const location = result.error_offset == null ? "" : ` Offset: ${result.error_offset}.`;
    setBanner("error", `${result.error ?? "Unknown compiler error."}${location}`);
  }
}

function parseResult(raw) {
  try {
    return JSON.parse(raw);
  } catch (error) {
    return {
      success: false,
      error: `Failed to parse compiler JSON: ${error}`,
      abi: null,
      entry_path: null,
    };
  }
}

async function runCompile(mode) {
  if (!wasmReady) {
    return;
  }

  setBusy(true);
  compileSummary.textContent = "Compiling";
  setBanner("neutral", "Compiler is running in WebAssembly...");

  try {
    const raw =
      mode === "project"
        ? compile_project(buildProjectPayload())
        : mode === "dargo"
          ? compile_dargo_project(buildDargoPayload())
        : compile_source(editor.value);
    renderResult(parseResult(raw));
  } catch (error) {
    renderResult({
      success: false,
      error: error instanceof Error ? error.message : String(error),
      abi: null,
      entry_path: null,
    });
  } finally {
    setBusy(false);
  }
}

function selectExample(name) {
  activeExample = name;
  editor.value = EXAMPLES[name];
  exampleButtons.forEach((button) => {
    button.dataset.active = button.dataset.example === name ? "true" : "false";
  });
  setBanner("neutral", `Loaded ${name} example. Click compile to run the wasm compiler.`);
  compileSummary.textContent = "Example loaded";
}

function buildProjectPayload() {
  if (activeExample === "module") {
    return JSON.stringify({
      entry: ["main"],
      method_names: ["main"],
      files: [
        [["main"], editor.value],
        [["foo"], `pub fn run() {}`],
      ],
    });
  }

  return JSON.stringify({
    entry: ["main"],
    method_names: ["main"],
    files: [[["main"], editor.value]],
  });
}

function buildDargoPayload() {
  const rootManifest = `[package]
name = "root"
type = "bin"`;

  const rootFiles =
    activeExample === "module"
      ? {
          "src/main.psy": editor.value,
          "src/foo.psy": "pub fn run() {}",
        }
      : {
          "src/main.psy": editor.value,
        };

  return JSON.stringify({
    root: "root",
    method_names: ["main"],
    packages: [
      {
        id: "root",
        manifest: rootManifest,
        files: rootFiles,
        dependencies: {},
      },
    ],
  });
}

exampleButtons.forEach((button) => {
  button.addEventListener("click", () => selectExample(button.dataset.example));
});

compileSourceButton.addEventListener("click", () => runCompile("source"));
compileProjectButton.addEventListener("click", () => runCompile("project"));
compileDargoButton.addEventListener("click", () => runCompile("dargo"));

async function boot() {
  const requestedExample = params.get("example");
  selectExample(EXAMPLES[requestedExample] ? requestedExample : "std");
  compileOutput.textContent = "Waiting for compile...";
  abiOutput.textContent = "Waiting for compile...";

  try {
    await init();
    init_logging();
    wasmReady = true;
    wasmStatus.textContent = "Ready";
    setBanner("neutral", "WASM module loaded. The demo is ready.");
    const autorun = params.get("autorun");
    if (autorun === "source" || autorun === "project" || autorun === "dargo") {
      await runCompile(autorun);
    }
  } catch (error) {
    wasmStatus.textContent = "Failed";
    compileSummary.textContent = "Unavailable";
    renderResult({
      success: false,
      error: error instanceof Error ? error.message : String(error),
      abi: null,
      entry_path: null,
    });
  }
}

boot();
