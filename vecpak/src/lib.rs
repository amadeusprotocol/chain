use num_bigint::{BigInt, Sign};

mod error;
mod ser;
mod de;
mod limits;
mod bigint;

use limits::Budget;

pub use limits::Limits;
pub use error::Error;
pub use ser::{to_vec, to_vec_with_limits};
pub use de::{from_slice, from_slice_with_limits};
pub use bigint::BigVarInt;

#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    Nil(),
    Bool(bool),
    VarInt(i128),
    BigInt(BigInt),
    Binary(Vec<u8>),
    List(Vec<Term>),
    PropList(Vec<(Term,Term)>),
}

pub const MAX_VARINT_BYTES: usize = 127;

#[inline(always)]
pub fn encode_varint(buf: &mut Vec<u8>, v: i128) {
    if v == 0 {
        buf.push(0);
        return;
    }

    let sign = (v < 0) as u8;
    let mag = v.unsigned_abs();
    let lz = mag.leading_zeros() as usize;
    let first = lz / 8;
    let len = 16 - first;
    buf.push((sign << 7) | (len as u8));
    let be = mag.to_be_bytes();
    buf.extend_from_slice(&be[first..]);
}

#[inline]
pub fn encode_varint_bytes(
    buf: &mut Vec<u8>,
    negative: bool,
    be_magnitude: &[u8],
) -> Result<(), &'static str> {
    if be_magnitude.is_empty() {
        buf.push(0);
        return Ok(());
    }
    if be_magnitude.len() > MAX_VARINT_BYTES {
        return Err("varint_too_large");
    }
    if be_magnitude[0] == 0 {
        return Err("varint_leading_zero");
    }
    buf.push(((negative as u8) << 7) | (be_magnitude.len() as u8));
    buf.extend_from_slice(be_magnitude);
    Ok(())
}

#[inline]
pub fn decode_varint_raw<'a>(
    buf: &'a [u8],
    i: &mut usize,
) -> Result<(bool, &'a [u8]), &'static str> {
    if *i >= buf.len() { return Err("eof"); }
    let b0 = buf[*i]; *i += 1;
    if b0 == 0 {
        return Ok((false, &[]));
    }
    if b0 == 0x80 { return Err("noncanonical_zero"); }

    let negative = (b0 & 0x80) != 0;
    let len = (b0 & 0x7F) as usize;
    if buf.len().saturating_sub(*i) < len { return Err("eof"); }
    if buf[*i] == 0 { return Err("varint_leading_zero"); }

    let mag = &buf[*i..*i + len];
    *i += len;
    Ok((negative, mag))
}

#[inline]
fn i128_from_raw(negative: bool, mag: &[u8]) -> Option<i128> {
    if mag.is_empty() {
        return Some(0);
    }
    if mag.len() > 16 {
        return None;
    }
    let mut be = [0u8; 16];
    be[16 - mag.len()..].copy_from_slice(mag);
    let m = u128::from_be_bytes(be);
    if negative {
        if m > (i128::MAX as u128) + 1 {
            None
        } else {
            Some((m as i128).wrapping_neg())
        }
    } else if m > i128::MAX as u128 {
        None
    } else {
        Some(m as i128)
    }
}

#[inline(always)]
pub fn decode_varint(buf: &[u8], i: &mut usize) -> Result<i128, &'static str> {
    let (negative, mag) = decode_varint_raw(buf, i)?;
    i128_from_raw(negative, mag).ok_or("varint_too_large_for_i128")
}

#[inline]
fn decode_varint_gte_zero(buf: &[u8], i: &mut usize) -> Result<usize, &'static str> {
    let n = decode_varint(buf, i)?;
    if n < 0 { return Err("length_is_negative"); }
    usize::try_from(n).map_err(|_| "length_overflow")
}

pub fn encode_term(buf: &mut Vec<u8>, term: Term) -> Result<(), &'static str> {
    let limits = Limits::default();
    let mut budget = Budget::new(&limits);
    let start = buf.len();
    let r = encode_term_limited(buf, term, &mut budget);
    if r.is_err() {
        buf.truncate(start);
    }
    r
}

fn encode_term_limited(
    buf: &mut Vec<u8>,
    term: Term,
    budget: &mut Budget,
) -> Result<(), &'static str> {
    match term {
        Term::Nil() => { buf.push(0); }
        Term::Bool(true) => { buf.push(1); }
        Term::Bool(false) => { buf.push(2); }
        Term::VarInt(varint) => {
            buf.push(3);
            encode_varint(buf, varint);
        }
        Term::BigInt(big) => {
            buf.push(3);
            let (sign, mag) = big.to_bytes_be();
            match sign {
                Sign::NoSign => buf.push(0),
                Sign::Minus => encode_varint_bytes(buf, true, &mag)?,
                Sign::Plus => encode_varint_bytes(buf, false, &mag)?,
            }
        }
        Term::Binary(bin) => {
            buf.push(5);
            encode_varint(buf, bin.len() as i128);
            buf.extend_from_slice(bin.as_slice());
        }
        Term::List(list) => {
            if list.len() > budget.limits.max_container_len {
                return Err("container_too_large");
            }
            budget.enter()?;
            buf.push(6);
            encode_varint(buf, list.len() as i128);
            for member in list {
                encode_term_limited(buf, member, budget)?;
            }
            budget.leave();
        }
        Term::PropList(proplist) => {
            if proplist.len() > budget.limits.max_container_len {
                return Err("container_too_large");
            }
            budget.enter()?;
            buf.push(7);
            encode_varint(buf, proplist.len() as i128);

            let mut keyed: Vec<(Vec<u8>, Term)> = Vec::with_capacity(proplist.len());
            for (k, v) in proplist {
                let mut kbytes = Vec::with_capacity(64);
                encode_term_limited(&mut kbytes, k, budget)?;
                keyed.push((kbytes, v));
            }
            keyed.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            if keyed.windows(2).any(|w| w[0].0 == w[1].0) {
                return Err("duplicate_map_key");
            }
            for (kbytes, v) in keyed {
                buf.extend_from_slice(&kbytes);
                encode_term_limited(buf, v, budget)?;
            }
            budget.leave();
        }
    }
    Ok(())
}

pub fn encode(term: Term) -> Result<Vec<u8>, &'static str> {
    encode_with_limits(term, &Limits::default())
}

pub fn encode_with_limits(term: Term, limits: &Limits) -> Result<Vec<u8>, &'static str> {
    let mut buf = Vec::with_capacity(1024);
    let mut budget = Budget::new(limits);
    encode_term_limited(&mut buf, term, &mut budget)?;
    Ok(buf)
}

#[inline]
fn read_u8(buf: &[u8], i: &mut usize) -> Result<u8, &'static str> {
    if *i >= buf.len() { return Err("eof"); }
    let b = buf[*i];
    *i += 1;
    Ok(b)
}

#[inline]
fn read_exact<'a>(buf: &'a [u8], i: &mut usize, n: usize) -> Result<&'a [u8], &'static str> {
    if buf.len().saturating_sub(*i) < n { return Err("eof"); }
    let s = &buf[*i..*i + n];
    *i += n;
    Ok(s)
}

pub fn decode_term(buf: &[u8], i: &mut usize) -> Result<Term, &'static str> {
    let limits = Limits::default();
    let mut budget = Budget::new(&limits);
    decode_term_limited(buf, i, &mut budget)
}

fn decode_term_limited(
    buf: &[u8],
    i: &mut usize,
    budget: &mut Budget,
) -> Result<Term, &'static str> {
    let tag = read_u8(buf, i)?;
    match tag {
        0 => { Ok(Term::Nil()) }
        1 => { Ok(Term::Bool(true)) }
        2 => { Ok(Term::Bool(false)) }
        3 => {
            let (negative, mag) = decode_varint_raw(buf, i)?;
            match i128_from_raw(negative, mag) {
                Some(v) => Ok(Term::VarInt(v)),
                None => {
                    let sign = if negative { Sign::Minus } else { Sign::Plus };
                    Ok(Term::BigInt(BigInt::from_bytes_be(sign, mag)))
                }
            }
        }
        5 => {
            let len = decode_varint_gte_zero(buf, i)?;
            let bytes = read_exact(buf, i, len)?.to_vec();
            Ok(Term::Binary(bytes))
        }
        6 => {
            let count = decode_varint_gte_zero(buf, i)?;
            let cap = budget.account_container(count, buf.len().saturating_sub(*i))?;
            budget.enter()?;
            let mut items = Vec::with_capacity(cap);
            for _ in 0..count {
                items.push(decode_term_limited(buf, i, budget)?);
            }
            budget.leave();
            Ok(Term::List(items))
        }
        7 => {
            let count = decode_varint_gte_zero(buf, i)?;
            let cap = budget.account_container(count, buf.len().saturating_sub(*i))?;
            budget.enter()?;
            let mut pairs = Vec::with_capacity(cap);

            //Canonical check
            let mut prev_key_bytes: Option<&[u8]> = None;

            for _ in 0..count {
                let k_start = *i;
                let k = decode_term_limited(buf, i, budget)?;
                let k_bytes = &buf[k_start..*i];

                if let Some(prev) = prev_key_bytes {
                    if k_bytes <= prev { return Err("map_not_canonical"); }
                }
                prev_key_bytes = Some(k_bytes);

                let v = decode_term_limited(buf, i, budget)?;
                pairs.push((k, v));
            }
            budget.leave();
            Ok(Term::PropList(pairs))
        }
        _ => Err("unknown_tag"),
    }
}

pub fn decode(buf: &[u8]) -> Result<Term, &'static str> {
    decode_with_limits(buf, &Limits::default())
}

pub fn decode_with_limits(buf: &[u8], limits: &Limits) -> Result<Term, &'static str> {
    let mut budget = Budget::new(limits);
    let mut i = 0;
    let term = decode_term_limited(buf, &mut i, &mut budget)?;
    if i != buf.len() { return Err("trailing_bytes"); }
    Ok(term)
}
