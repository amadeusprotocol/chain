const TYPE_NULL = 0x00;
const TYPE_TRUE = 0x01;
const TYPE_FALSE = 0x02;
const TYPE_INT = 0x03;
const TYPE_BYTES = 0x05;
const TYPE_LIST = 0x06;
const TYPE_MAP = 0x07;

export function encode(term) {
  const bytesOut = [];
  encodeTerm(term, bytesOut);
  return new Uint8Array(bytesOut);
}

export function decode(bytes) {
  const data = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  const ref = { offset: 0 };
  const value = decodeTerm(data, ref);
  if (ref.offset !== data.length) {
    throw new Error("trailing_bytes");
  }
  return value;
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
  return a.length - b.length; // shorter wins if prefix equal
}

function encodeKeyBytes(k) {
  const tmp = [];
  encodeTerm(k, tmp);
  return tmp;
}

function encodeVarint(n, out) {
  let value = typeof n === "bigint" ? n : window.BigInt(n);

  if (value === 0n) {
    out.push(0);
    return;
  }

  const isNegative = value < 0n;
  if (isNegative) value = -value;

  // build big-endian magnitude
  let magBytes = [];
  while (value > 0n) {
    magBytes.push(Number(value & 0xffn));
    value >>= 8n;
  }
  magBytes.reverse(); // now big-endian

  const len = magBytes.length;
  if (len === 0 || len > 16) {
    throw new Error("bad_varint_length");
  }

  // Rust also rejects leading zero in decode; we don't generate those here
  if (magBytes[0] === 0) {
    throw new Error("varint_leading_zero");
  }

  const header = ((isNegative ? 1 : 0) << 7) | len;
  out.push(header);
  appendBytes(out, magBytes);
}

function decodeVarint(data, ref) {
  const header = data[ref.offset++];
  if (header === 0) return 0n;
  if (header === 0x80) throw new Error("noncanonical_zero");

  const signBit = header >> 7;
  const length = header & 0x7f;

  let mag = 0n;
  for (let i = 0; i < length; i++) {
    mag = (mag << 8n) | window.BigInt(data[ref.offset++]);
  }

  if (mag > BigInt(Number.MAX_SAFE_INTEGER)) throw new Error("length_overflow");

  if (signBit === 1) {
    return -mag;
  } else {
    return mag;
  }
}

function decodeVarintGteZero(data, ref) {
  const n = decodeVarint(data, ref);
  if (n < 0n) {
    throw new Error("length_is_negative");
  }
  return Number(n);
}

function encodeTerm(value, out) {
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
    encodeVarint(utf8.length, out);
    appendBytes(out, utf8);
  } else if (value instanceof Uint8Array) {
    out.push(TYPE_BYTES);
    encodeVarint(value.length, out);
    appendBytes(out, value);
  } else if (Array.isArray(value)) {
    out.push(TYPE_LIST);
    encodeVarint(value.length, out);
    for (const element of value) {
      encodeTerm(element, out);
    }
  } else if (
    value instanceof Map ||
    (typeof value === "object" &&
      !Array.isArray(value) &&
      !(value instanceof Uint8Array))
  ) {
    const entries = [];
    if (value instanceof Map) {
      for (const [k, v] of value.entries()) {
        const bytes = encodeKeyBytes(k); // encodes key as Term
        entries.push({ k, v, bytes });
      }
    } else {
      for (const k of Object.keys(value)) {
        const v = value[k];
        const bytes = encodeKeyBytes(k); // k is string; still encoded via encodeValue
        entries.push({ k, v, bytes });
      }
    }
    entries.sort((a, b) => compareBytes(a.bytes, b.bytes));

    out.push(TYPE_MAP);
    encodeVarint(entries.length, out);
    for (const entry of entries) {
      encodeTerm(entry.k, out);
      encodeTerm(entry.v, out);
    }
  } else {
    throw new Error(`Unsupported type: ${typeof value}`);
  }
}

function decodeTerm(data, ref) {
  if (ref.offset >= data.length) {
    throw new Error("decodeByes: Out of bounds read");
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
    case TYPE_BYTES:
      const length = decodeVarint(data, ref);
      const bytes = data.slice(ref.offset, ref.offset + length);
      ref.offset += length;
      return bytes;
    case TYPE_LIST: {
      const count = decodeVarintGteZero(data, ref);
      const items = new Array(count);
      for (let i = 0; i < count; i++) {
        items[i] = decodeTerm(data, ref);
      }
      return items;
    }
    case TYPE_MAP: {
      const count = decodeVarintGteZero(data, ref);

      let prevKeyBytes = null;
      const map = new Map();

      for (let idx = 0; idx < count; idx++) {
        // canonical check: track raw key bytes
        const kStart = ref.offset;
        const key = decodeTerm(data, ref);
        const kEnd = ref.offset;
        const keyBytes = data.slice(kStart, kEnd);

        if (prevKeyBytes !== null) {
          if (compareBytes(keyBytes, prevKeyBytes) <= 0) {
            throw new Error("map_not_canonical");
          }
        }
        prevKeyBytes = keyBytes;

        const value = decodeTerm(data, ref);
        map.set(key, value);
      }
      return map;
    }
    default:
      throw new Error("decodeBytes: Unknown type");
  }
}
