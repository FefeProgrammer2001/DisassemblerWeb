import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const wasmPath = path.join(__dirname, 'target/wasm32-unknown-unknown/release/c_compiler_wasm.wasm');
const wasmBytes = fs.readFileSync(wasmPath);

const wasmModule = await WebAssembly.instantiate(wasmBytes, {});
const exports = wasmModule.instance.exports;

export function compileC(cSource) {
  const encoder = new TextEncoder();
  const sourceBytes = encoder.encode(cSource);

  // Allocate source memory in WASM
  const sourcePtr = exports.wasm_alloc(sourceBytes.length);
  const memView = new Uint8Array(exports.memory.buffer);
  memView.set(sourceBytes, sourcePtr);

  // Call compiler
  const resPtr = exports.compile_c(sourcePtr, sourceBytes.length);

  // Read response [4-byte len] + [UTF-8 JSON bytes]
  const dataView = new DataView(exports.memory.buffer);
  const jsonLen = dataView.getUint32(resPtr, true); // little-endian
  const jsonBytes = new Uint8Array(exports.memory.buffer, resPtr + 4, jsonLen);
  const jsonStr = new TextDecoder().decode(jsonBytes);

  // Free memory
  exports.wasm_free(sourcePtr, sourceBytes.length);
  exports.wasm_free(resPtr, jsonLen + 4);

  return JSON.parse(jsonStr);
}

// Test case 1: Recursion & Local Pointers
const testCode1 = `
int factorial(int n) {
    if (n <= 1) {
        return 1;
    }
    return n * factorial(n - 1);
}

int main() {
    int x = 5;
    int *p = &x;
    int ans = factorial(*p);
    return ans;
}
`;

console.log("=== Testing WASM Compilation with Recursion & Pointers ===");
const res1 = compileC(testCode1);
console.log("Success:", res1.success);
console.log("\n--- Functions detected ---");
for (const fn of res1.functions) {
  console.log(`Function '${fn.name}' (Stack frame: ${fn.stack_frame_size} bytes):`);
  console.log("  Params:", fn.params.map(p => `${p.name} (${p.type_name}) -> ${p.register_or_stack}`).join(", ") || "none");
  console.log("  Locals:", fn.locals.map(l => `${l.name} (${l.type_name}, ${l.size}B) at [rbp${l.rbp_offset}]`).join(", ") || "none");
}

console.log("\n--- Sample Assembly Lines with Metadata ---");
for (const line of res1.lines.slice(0, 25)) {
  const comment = line.comment ? ` # ${line.comment}` : "";
  const src = line.source_line ? ` [C line ${line.source_line}]` : "";
  console.log(`${line.text.padEnd(35)}${comment}${src}`);
}

// Test case 2: While loop with compound assignment
const testCode2 = `
int sum_array() {
    int sum = 0;
    int i = 1;
    while (i <= 10) {
        sum += i;
        i++;
    }
    return sum;
}
`;

console.log("\n=== Testing Loop Compilation ===");
const res2 = compileC(testCode2);
console.log("Success:", res2.success);
console.log("Generated assembly length:", res2.lines.length, "lines");
console.log("Loop instructions include while_start and while_end labels:", res2.assembly.includes(".L.while_start"));

// Test case 3: Arrays
const testCode3 = `
int test_arrays() {
    int arr[3];
    arr[0] = 10;
    arr[1] = 20;
    arr[2] = 30;
    return arr[0] + arr[1] + arr[2];
}
`;

console.log("\n=== Testing Array Compilation ===");
const res3 = compileC(testCode3);
console.log("Success:", res3.success);
console.log("Locals:", res3.functions[0].locals.map(l => `${l.name} (${l.type_name}, ${l.size}B) at [rbp${l.rbp_offset}]`).join(", "));

// Test case 4: Syntax error reporting
const testCodeError = `
int broken() {
    int x = ;
}
`;

console.log("\n=== Testing Error Reporting ===");
const resError = compileC(testCodeError);
console.log("Success:", resError.success);
console.log("Errors captured:", resError.errors);

// Test case 5: Structs & Member Access
const testCode5 = `
struct Point {
    int x;
    int y;
};

int test_struct() {
    struct Point pt;
    pt.x = 10;
    pt.y = 25;
    struct Point *ptr = &pt;
    ptr->x += 5;
    return ptr->x + ptr->y;
}
`;

console.log("\n=== Testing Struct & Member Access Compilation ===");
const res5 = compileC(testCode5);
console.log("Success:", res5.success);
console.log("Locals:", res5.functions[0].locals.map(l => `${l.name} (${l.type_name}, ${l.size}B) at [rbp${l.rbp_offset}]`).join(", "));
console.log("Assembly includes member offset add instructions:", res5.assembly.includes("add rax, 4"));

if (!res1.success || !res2.success || !res3.success || resError.success || !res5.success) {
  console.error("Test failed!");
  process.exit(1);
}

// Stepper Engine JS Wrappers
export function createStepper(cSource) {
  const encoder = new TextEncoder();
  const sourceBytes = encoder.encode(cSource);
  const sourcePtr = exports.wasm_alloc(sourceBytes.length);
  const memView = new Uint8Array(exports.memory.buffer);
  memView.set(sourceBytes, sourcePtr);

  const handle = exports.stepper_create(sourcePtr, sourceBytes.length);
  exports.wasm_free(sourcePtr, sourceBytes.length);
  if (!handle) throw new Error("Failed to create stepper");
  return handle;
}

export function stepperStep(handle) {
  const resPtr = exports.stepper_step(handle);
  const dataView = new DataView(exports.memory.buffer);
  const jsonLen = dataView.getUint32(resPtr, true);
  const jsonBytes = new Uint8Array(exports.memory.buffer, resPtr + 4, jsonLen);
  const jsonStr = new TextDecoder().decode(jsonBytes);
  exports.wasm_free(resPtr, jsonLen + 4);
  return JSON.parse(jsonStr);
}

export function stepperStepBack(handle) {
  return exports.stepper_step_back(handle) === 1;
}

export function stepperFree(handle) {
  exports.stepper_free(handle);
}

// Test case 6: CPU & Memory Stepper Engine in WebAssembly
console.log("\n=== Testing CPU & Memory Stepper Engine in WASM ===");
const stepperCode = `
int add(int a, int b) {
    return a + b;
}

int main() {
    int x = 15;
    int y = 25;
    return add(x, y);
}
`;

const handle = createStepper(stepperCode);
let steps = 0;
let lastStep = null;
const memoryEvents = [];
const registerChanges = [];

while (steps < 200) {
  const stepRes = stepperStep(handle);
  if (stepRes.memory_events && stepRes.memory_events.length > 0) {
    memoryEvents.push(...stepRes.memory_events);
  }
  if (stepRes.register_changes && stepRes.register_changes.length > 0) {
    registerChanges.push(...stepRes.register_changes);
  }
  if (stepRes.is_halted) {
    lastStep = stepRes;
    break;
  }
  steps += 1;
}

console.log("Stepper completed in", steps, "steps");
console.log("Program return value:", lastStep?.return_value);
console.log("Memory access events recorded:", memoryEvents.length);
if (memoryEvents.length > 0) {
  console.log("Sample memory event:", memoryEvents[0]);
}
console.log("Register changes recorded:", registerChanges.length);

// Test step back
const steppedBack = stepperStepBack(handle);
console.log("Step back successful:", steppedBack);
stepperFree(handle);

if (lastStep?.return_value !== 40 || !steppedBack) {
  console.error("Stepper WASM test failed!");
  process.exit(1);
} else {
  console.log("\nAll WASM tests (Compiler + Stepper Engine) passed successfully!");
}

