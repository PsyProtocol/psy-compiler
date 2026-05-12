const wasm = require("../pkg-node/psy_wasm.js");

function parseResult(raw) {
  try {
    return JSON.parse(raw);
  } catch (error) {
    throw new Error(`Failed to parse wasm JSON result: ${error}\nRaw: ${raw}`);
  }
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function run() {
  const single = parseResult(
    wasm.compile_source('fn main() { let a = 1; assert_eq(a, 1, "ok"); }'),
  );
  assert(single.success === true, `single-file compile failed: ${single.error}`);

  const multi = parseResult(
    wasm.compile_project(
      JSON.stringify([
        [["main"], "mod foo;\nfn main() { foo::run(); }"],
        [["foo"], "pub fn run() {}"],
      ]),
    ),
  );
  assert(multi.success === true, `multi-file compile failed: ${multi.error}`);

  const parseError = parseResult(wasm.compile_source("fn main( { }"));
  assert(parseError.success === false, "parse-error case unexpectedly succeeded");
  assert(
    typeof parseError.error_offset === "number",
    `expected numeric error_offset, got: ${parseError.error_offset}`,
  );

  console.log("single:", JSON.stringify(single));
  console.log("multi:", JSON.stringify(multi));
  console.log("parse_error:", JSON.stringify(parseError));
}

run();
