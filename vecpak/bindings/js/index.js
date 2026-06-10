const TYPE_NULL = 0x00;
const TYPE_TRUE = 0x01;
const TYPE_FALSE = 0x02;
const TYPE_INT = 0x03;
const TYPE_BYTES = 0x05;
const TYPE_LIST = 0x06;
const TYPE_MAP = 0x07;

const MAX_VARINT_BYTES = 127;

const DEFAULT_LIMITS = {
  maxDepth: 16,
  maxContainerLen: 16_777_216,
};

const LIMIT_KEYS = Object.keys(DEFAULT_LIMITS);

function resolveLimits(opts) {
  if (opts == null) return DEFAULT_LIMITS;
  const get = opts instanceof Map ? (k) => opts.get(k) : (k) => opts[k];
  const keys = opts instanceof Map ? [...opts.keys()] : Object.keys(opts);
  for (const k of keys) {
    if (!LIMIT_KEYS.includes(k)) {
      throw new Error(`unknown limit option: ${k}`);
    }
  }
  const limits = { ...DEFAULT_LIMITS };
  for (const k of LIMIT_KEYS) {
    const v = get(k);
    if (v !== undefined) {
      const n = Number(v);
      if (!Number.isInteger(n) || n < 0) {
        throw new Error(`limit ${k} must be a non-negative integer`);
      }
      limits[k] = n;
    }
  }
  return limits;
}

export function encode(term, opts) {
  const limits = resolveLimits(opts);
  const bytesOut = [];
  encodeTerm(term, bytesOut, limits, 0);
  return new Uint8Array(bytesOut);
}

export function decode(bytes, opts) {
  const limits = resolveLimits(opts);
  const data = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  const ref = { offset: 0 };
  const value = decodeTerm(data, ref, limits, 0);
  if (ref.offset !== data.length) {
    throw new Error("trailing_bytes");
  }
  return value;
}

export function toObject(value, { keysAsUtf8 = false } = {}) {
  if (Array.isArray(value)) return value.map((v) => toObject(v, { keysAsUtf8 }));
  if (value instanceof Map) {
    const obj = {};
    for (const [k, v] of value) {
      const decoded = keysAsUtf8 && k instanceof Uint8Array ? new TextDecoder().decode(k) : k;
      const key = String(decoded);
      if (Object.prototype.hasOwnProperty.call(obj, key)) {
        throw new Error(`toObject: map keys collide on property "${key}"`);
      }
      Object.defineProperty(obj, key, {
        value: toObject(v, { keysAsUtf8 }),
        enumerable: true,
        writable: true,
        configurable: true,
      });
    }
    return obj;
  }
  return value;
}

export function decodeToObject(bytes, opts = {}) {
  let keysAsUtf8 = false;
  let limitOpts = opts;
  if (opts instanceof Map) {
    keysAsUtf8 = opts.get("keysAsUtf8") ?? false;
    // strip keysAsUtf8 so decode/resolveLimits still validates the rest
    limitOpts = new Map([...opts].filter(([k]) => k !== "keysAsUtf8"));
  } else if (opts) {
    ({ keysAsUtf8 = false, ...limitOpts } = opts);
  }
  return toObject(decode(bytes, limitOpts), { keysAsUtf8 });
}

function appendBytes(out, bytes) {
  for (const b of bytes) {
    out.push(b);
  }
}

function compareBytes(a, b) {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    if (a[i] !== b[i]) return a[i] - b[i];
  }
  return a.length - b.length;
}

function encodeKeyBytes(k, limits, depth) {
  const tmp = [];
  encodeTerm(k, tmp, limits, depth);
  return tmp;
}

function encodeVarint(n, out) {
  if (typeof n === "number" && !Number.isSafeInteger(n)) {
    throw new Error("unsafe_integer: pass a BigInt for values outside +/-2^53");
  }
  let value = typeof n === "bigint" ? n : BigInt(n);

  if (value === 0n) {
    out.push(0);
    return;
  }

  const isNegative = value < 0n;
  if (isNegative) value = -value;

  let magBytes = [];
  while (value > 0n) {
    magBytes.push(Number(value & 0xffn));
    value >>= 8n;
  }
  magBytes.reverse();

  const len = magBytes.length;
  if (len === 0 || len > MAX_VARINT_BYTES) {
    throw new Error("bad_varint_length");
  }

  if (magBytes[0] === 0) {
    throw new Error("varint_leading_zero");
  }

  const header = ((isNegative ? 1 : 0) << 7) | len;
  out.push(header);
  appendBytes(out, magBytes);
}

function decodeVarint(data, ref) {
  if (ref.offset >= data.length) throw new Error("eof");
  const header = data[ref.offset++];
  if (header === 0) return 0n;
  if (header === 0x80) throw new Error("noncanonical_zero");

  const signBit = header >> 7;
  const length = header & 0x7f;
  if (ref.offset + length > data.length) throw new Error("eof");
  if (data[ref.offset] === 0) throw new Error("varint_leading_zero");

  let mag = 0n;
  for (let i = 0; i < length; i++) {
    mag = (mag << 8n) | BigInt(data[ref.offset++]);
  }

  return signBit === 1 ? -mag : mag;
}

function decodeLength(data, ref) {
  const n = decodeVarint(data, ref);
  if (n < 0n) throw new Error("length_is_negative");
  return n;
}

function encodeTerm(value, out, limits, depth) {
  if (value === null) {
    out.push(TYPE_NULL);
  } else if (typeof value === "boolean") {
    out.push(value ? TYPE_TRUE : TYPE_FALSE);
  } else if (typeof value === "number" || typeof value === "bigint") {
    out.push(TYPE_INT);
    encodeVarint(value, out);
  } else if (typeof value === "string") {
    out.push(TYPE_BYTES);
    const utf8 = new TextEncoder().encode(value);
    const wellFormed =
      typeof value.isWellFormed === "function"
        ? value.isWellFormed()
        : new TextDecoder().decode(utf8) === value;
    if (!wellFormed) {
      throw new Error("lone_surrogate: string is not well-formed UTF-16");
    }
    encodeVarint(utf8.length, out);
    appendBytes(out, utf8);
  } else if (value instanceof Uint8Array) {
    out.push(TYPE_BYTES);
    encodeVarint(value.length, out);
    appendBytes(out, value);
  } else if (Array.isArray(value)) {
    if (depth + 1 > limits.maxDepth) throw new Error("depth_limit_exceeded");
    if (value.length > limits.maxContainerLen) throw new Error("container_too_large");
    out.push(TYPE_LIST);
    encodeVarint(value.length, out);
    for (const element of value) {
      encodeTerm(element, out, limits, depth + 1);
    }
  } else if (
    value instanceof Map ||
    (typeof value === "object" &&
      !Array.isArray(value) &&
      !(value instanceof Uint8Array))
  ) {
    if (depth + 1 > limits.maxDepth) throw new Error("depth_limit_exceeded");
    const entries = [];
    if (value instanceof Map) {
      for (const [k, v] of value.entries()) {
        const bytes = encodeKeyBytes(k, limits, depth + 1);
        entries.push({ k, v, bytes });
      }
    } else {
      const proto = Object.getPrototypeOf(value);
      if (proto !== Object.prototype && proto !== null) {
        throw new Error(`Unsupported type: ${value?.constructor?.name ?? "object"}`);
      }
      for (const k of Object.keys(value)) {
        const v = value[k];
        const bytes = encodeKeyBytes(k, limits, depth + 1);
        entries.push({ k, v, bytes });
      }
    }
    if (entries.length > limits.maxContainerLen) throw new Error("container_too_large");
    entries.sort((a, b) => compareBytes(a.bytes, b.bytes));

    out.push(TYPE_MAP);
    encodeVarint(entries.length, out);
    let prevBytes = null;
    for (const entry of entries) {
      if (prevBytes !== null && compareBytes(prevBytes, entry.bytes) >= 0) {
        throw new Error("map_not_canonical");
      }
      prevBytes = entry.bytes;
      encodeTerm(entry.k, out, limits, depth + 1);
      encodeTerm(entry.v, out, limits, depth + 1);
    }
  } else {
    throw new Error(`Unsupported type: ${typeof value}`);
  }
}

function decodeTerm(data, ref, limits, depth) {
  if (ref.offset >= data.length) {
    throw new Error("decodeBytes: Out of bounds read");
  }

  const type = data[ref.offset++];

  switch (type) {
    case TYPE_NULL:
      return null;
    case TYPE_TRUE:
      return true;
    case TYPE_FALSE:
      return false;
    case TYPE_INT:
      return decodeVarint(data, ref);
    case TYPE_BYTES: {
      const lenB = decodeLength(data, ref);
      if (lenB > BigInt(data.length - ref.offset)) throw new Error("eof");
      const length = Number(lenB);
      const bytes = data.slice(ref.offset, ref.offset + length);
      ref.offset += length;
      return bytes;
    }
    case TYPE_LIST: {
      const count = decodeCount(data, ref, limits);
      if (depth + 1 > limits.maxDepth) throw new Error("depth_limit_exceeded");
      const items = [];
      for (let i = 0; i < count; i++) {
        items.push(decodeTerm(data, ref, limits, depth + 1));
      }
      return items;
    }
    case TYPE_MAP: {
      const count = decodeCount(data, ref, limits);
      if (depth + 1 > limits.maxDepth) throw new Error("depth_limit_exceeded");

      let prevKeyBytes = null;
      const map = new Map();

      for (let idx = 0; idx < count; idx++) {
        // canonical check: track raw key bytes
        const kStart = ref.offset;
        const key = decodeTerm(data, ref, limits, depth + 1);
        const kEnd = ref.offset;
        const keyBytes = data.slice(kStart, kEnd);

        if (prevKeyBytes !== null) {
          if (compareBytes(keyBytes, prevKeyBytes) <= 0) {
            throw new Error("map_not_canonical");
          }
        }
        prevKeyBytes = keyBytes;

        const value = decodeTerm(data, ref, limits, depth + 1);
        map.set(key, value);
      }
      return map;
    }
    default:
      throw new Error("decodeBytes: Unknown type");
  }
}

function decodeCount(data, ref, limits) {
  const countB = decodeLength(data, ref);
  if (countB > BigInt(limits.maxContainerLen)) throw new Error("container_too_large");
  if (countB > BigInt(data.length - ref.offset)) throw new Error("count_exceeds_input");
  return Number(countB);
}
