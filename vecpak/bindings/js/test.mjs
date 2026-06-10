// Regression + cross-binding tests for the JS vecpak binding.
// Run with:  node test.mjs
//
// The "enc *" vectors are produced by the Rust reference encoder
// (`vecpak::to_vec`) and asserted byte-for-byte, so this also guards
// cross-implementation agreement (a divergence here is a consensus bug).

import { encode, decode } from "./index.js";

const hex = (u) => Buffer.from(u).toString("hex");
const fromHex = (h) => new Uint8Array(h.match(/../g).map((b) => parseInt(b, 16)));
const mapObj = (m) => {
  const o = {};
  for (const [k, v] of m)
    o[Buffer.from(k).toString("utf8")] = v instanceof Uint8Array ? Buffer.from(v).toString("utf8") : v;
  return o;
};

let pass = 0, fail = 0;
const eq = (name, got, want) => {
  if (got === want) pass++;
  else { fail++; console.log(`FAIL ${name}: got ${got} want ${want}`); }
};
const throws = (name, fn, msg) => {
  try { fn(); fail++; console.log(`FAIL ${name}: expected throw ${msg}`); }
  catch (e) {
    if (msg && !e.message.includes(msg)) { fail++; console.log(`FAIL ${name}: threw "${e.message}" want "${msg}"`); }
    else pass++;
  }
};

// ---- cross-binding: JS encode must equal Rust to_vec byte-for-byte ----
eq("enc int30", hex(encode(30)), "03011e");
eq("enc strAlice", hex(encode("Alice")), "050105416c696365");
eq("enc negbig", hex(encode(-1234567890123456789n)), "0388112210f47de98115");
eq("enc i128min", hex(encode(-(1n << 127n))), "039080000000000000000000000000000000");
eq("enc person", hex(encode({ name: "Alice", age: 30, active: true })),
  "07010305010361676503011e0501046e616d65050105416c69636505010661637469766501");
eq("enc map_ab", hex(encode(new Map([["b", 1], ["a", 2]]))), "0701020501016103010205010162030101");

// ---- cross-binding: JS decode of Rust-produced bytes ----
eq("dec int30", decode(fromHex("03011e")), 30n);
eq("dec i128min", decode(fromHex("039080000000000000000000000000000000")), -(1n << 127n));
const person = mapObj(decode(fromHex(
  "07010305010361676503011e0501046e616d65050105416c69636505010661637469766501")));
eq("dec person.age", person.age, 30n);
eq("dec person.name", person.name, "Alice");
eq("dec person.active", person.active, true);

// ---- round-trips ----
for (const [n, v] of [["null", null], ["true", true], ["false", false],
  ["bigpos", 123456789012345678901234567890n], ["neg", -42n]])
  eq(`rt ${n}`, String(decode(encode(v))), String(v));
eq("rt bytes", hex(decode(encode(new Uint8Array([1, 2, 3, 255])))), "010203ff");
eq("rt i128min", decode(encode(-(1n << 127n))), -(1n << 127n));
eq("rt big>2^53", decode(encode(9007199254740993n)), 9007199254740993n);

// arbitrary-precision varints: the format allows magnitudes up to 127 bytes,
// well beyond i128 (e.g. a 512-bit hash/key carried as a number)
const k512 = (1n << 511n) + 0xdeadbeefn;
eq("rt 512-bit", decode(encode(k512)), k512);
eq("rt -512-bit", decode(encode(-k512)), -k512);
const k1000 = (1n << 1000n) - 1n; // 125-byte magnitude, within the cap
eq("rt 1000-bit", decode(encode(k1000)), k1000);
throws("over 127-byte magnitude errors", () => encode(1n << 1016n), "bad_varint_length");

// ---- security / canonical rejections (must match the Rust reference) ----
let deep = new Uint8Array([0]);
for (let i = 0; i < 100000; i++) {
  const w = new Uint8Array(deep.length + 3);
  w[0] = 6; w[1] = 1; w[2] = 1; w.set(deep, 3); deep = w;
}
throws("deep depth", () => decode(deep), "depth_limit_exceeded");
throws("huge count", () => decode(fromHex("0606010000000000")), "container_too_large");
throws("lying count", () => decode(fromHex("060203e8")), "count_exceeds_input");
throws("varint leading zero", () => decode(fromHex("03020005")), "varint_leading_zero");
throws("noncanonical zero", () => decode(fromHex("0380")), "noncanonical_zero");
throws("map order", () => decode(fromHex("0701020501016203010105010161030102")), "map_not_canonical");
throws("map dup", () => decode(fromHex("0701020501016103010105010161030102")), "map_not_canonical");
throws("unsafe int encode", () => encode(9007199254740993), "unsafe_integer");
throws("dup key encode", () => encode(new Map([[new Uint8Array([1]), 1], [new Uint8Array([1]), 2]])), "map_not_canonical");
throws("unknown opt", () => decode(fromHex("00"), { maxDpeth: 5 }), "unknown limit option");
throws("removed opt rejected", () => decode(fromHex("00"), { maxInputLen: 10 }), "unknown limit option");
eq("bytes no TypeError", hex(decode(encode("hi"))), "6869");
// a large binary now decodes (no maxBinaryLen) — bounded only by the input itself
eq("large binary ok", decode(encode(new Uint8Array(100000))).length, 100000);

console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
