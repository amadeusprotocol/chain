use rustler::types::{tuple, map::{MapIterator}, BigInt, Binary, OwnedBinary};
use rustler::{
    Atom, Decoder, Encoder, Env, Error, Term, TermType
};
use num_bigint::Sign;
use vecpak::{ encode_varint, encode_varint_bytes, decode_varint, decode_varint_raw, Limits };

pub use vecpak::Limits as VecpakLimits;

const PREALLOC_CAP: usize = 4096;

pub fn parse_limits(opts: Term) -> Result<Limits, Error> {
    let mut limits = Limits::default();
    let iter = opts
        .decode::<MapIterator>()
        .map_err(|_| Error::Atom("opts_not_a_map"))?;
    for (k, v) in iter {
        let key = k
            .atom_to_string()
            .or_else(|_| {
                Binary::from_term(k).map(|b| String::from_utf8_lossy(b.as_slice()).into_owned())
            })
            .map_err(|_| Error::Atom("opt_key_not_atom_or_string"))?;
        let val = v
            .decode::<i64>()
            .map_err(|_| Error::Atom("opt_value_not_integer"))?;
        if val < 0 {
            return Err(Error::Atom("opt_value_negative"));
        }
        limits
            .set(&key, val as usize)
            .map_err(|_| Error::Atom("unknown_opt"))?;
    }
    Ok(limits)
}

#[inline(always)]
fn decode_varint_ex(buf: &[u8], i: &mut usize) -> Result<i128, Error> {
    decode_varint(buf, i).map_err(|e| Error::Atom(e))
}

pub fn encode_term(env: Env, buf: &mut Vec<u8>, term: Term, depth: usize) -> Result<(), Error> {
    // ---- nil (tag 0) ----
    if rustler::types::atom::nil().eq(&term) {
        buf.push(0);
        return Ok(())
    }
    // ---- bool (tag 1 | 2) ----
    if let Ok(b) = term.decode::<bool>() {
        if b { buf.push(1); } else { buf.push(2); };
        return Ok(())
    }
    // ---- VarInt (tag 3) ----
    if term.get_type() == TermType::Integer {
        if let Ok(bi) = BigInt::decode(term) {
            buf.push(3);
            let (sign, mag) = bi.to_bytes_be();
            match sign {
                Sign::NoSign => buf.push(0),
                Sign::Minus => encode_varint_bytes(buf, true, &mag).map_err(Error::Atom)?,
                Sign::Plus => encode_varint_bytes(buf, false, &mag).map_err(Error::Atom)?,
            }
            return Ok(());
        }
    }
    // ---- Binary (tag 5) OR Atom (tag 5) ----
    if let Ok(bin) = Binary::from_term(term) {
        buf.push(5);
        encode_varint(buf, bin.len() as i128);
        buf.extend_from_slice(bin.as_slice());
        return Ok(());
    }
    if let Ok(atom_string) = term.atom_to_string() {
        buf.push(5);
        encode_varint(buf, atom_string.len() as i128);
        buf.extend_from_slice(atom_string.as_bytes());
        return Ok(());
    }

    // ---- Map (tag 7) ----
    if let Ok(iter) = term.decode::<MapIterator>() {
        if depth + 1 > Limits::default().max_depth {
            return Err(Error::Atom("depth_limit_exceeded"));
        }
        if term.map_size()? > Limits::default().max_container_len {
            return Err(Error::Atom("container_too_large"));
        }
        buf.push(7);
        encode_varint(buf, term.map_size()? as i128);

        let mut keyed: Vec<(Vec<u8>, Term)> = Vec::with_capacity(iter.size_hint().0);
        for (k, v) in iter {
            let mut kbytes = Vec::with_capacity(32);
            encode_term(env, &mut kbytes, k, depth + 1)?;
            keyed.push((kbytes, v));
        }
        keyed.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        if keyed.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(Error::Atom("duplicate_map_key"));
        }
        for (kbytes, v) in keyed {
            buf.extend_from_slice(&kbytes);
            encode_term(env, buf, v, depth + 1)?;
        }
        return Ok(());
    }

    // ---- PropList (tag 7) ----
    if let Ok(mut it) = term.into_list_iterator() {
        if depth + 1 > Limits::default().max_depth {
            return Err(Error::Atom("depth_limit_exceeded"));
        }
        if (term.list_length()? as usize) > Limits::default().max_container_len {
            return Err(Error::Atom("container_too_large"));
        }
        let mut tmp_pairs: Vec<(Term, Term)> = Vec::new();
        let mut is_proplist = term.list_length()? > 0;

        while let Some(elem) = it.next() {
            if let Ok((k, v)) = elem.decode::<(Term, Term)>() {
                tmp_pairs.push((k, v));
            } else {
                is_proplist = false;
                break;
            }
        }

        if is_proplist {
            buf.push(7);
            encode_varint(buf, term.list_length()? as i128);

            let mut keyed: Vec<(Vec<u8>, Term)> = Vec::with_capacity(tmp_pairs.len());
            for (k, v) in tmp_pairs {
                let mut kbytes = Vec::with_capacity(32);
                encode_term(env, &mut kbytes, k, depth + 1)?;
                keyed.push((kbytes, v));
            }
            keyed.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            if keyed.windows(2).any(|w| w[0].0 == w[1].0) {
                return Err(Error::Atom("duplicate_map_key"));
            }
            for (kbytes, v) in keyed {
                buf.extend_from_slice(&kbytes);
                encode_term(env, buf, v, depth + 1)?;
            }
            return Ok(());
        } else {
            buf.push(6);
            encode_varint(buf, term.list_length()? as i128);
            let it = term.into_list_iterator().expect("list_iterator");
            for v in it {
                encode_term(env, buf, v, depth + 1)?;
            }
            return Ok(());
        }
    }

    // ---- Tuple (encode as list 6) ----
    if let Ok(tpl) = tuple::get_tuple(term) {
        if depth + 1 > Limits::default().max_depth {
            return Err(Error::Atom("depth_limit_exceeded"));
        }
        if tpl.len() > Limits::default().max_container_len {
            return Err(Error::Atom("container_too_large"));
        }
        buf.push(6);
        encode_varint(buf, tpl.len() as i128);
        for v in tpl {
            encode_term(env, buf, v, depth + 1)?;
        }
        return Ok(());
    }

    Err(Error::BadArg)
}

#[inline]
fn decode_varint_gt_zero(buf: &[u8], i: &mut usize) -> Result<usize, Error> {
    let n = decode_varint_ex(buf, i)?;
    if n < 0 { return Err(Error::Atom("length_is_negative")); }
    usize::try_from(n).map_err(|_| Error::Atom("length_overflow"))
}

#[inline]
fn read_u8(buf: &[u8], i: &mut usize) -> Result<u8, Error> {
    if *i >= buf.len() { return Err(Error::Atom("eof")); }
    let b = buf[*i];
    *i += 1;
    Ok(b)
}

#[inline]
fn read_exact<'a>(buf: &'a [u8], i: &mut usize, n: usize) -> Result<&'a [u8], Error> {
    if buf.len().saturating_sub(*i) < n { return Err(Error::Atom("eof")); }
    let s = &buf[*i..*i + n];
    *i += n;
    Ok(s)
}

#[inline]
fn account_container(count: usize, remaining: usize, limits: &Limits) -> Result<usize, Error> {
    if count > limits.max_container_len {
        return Err(Error::Atom("container_too_large"));
    }
    if count > remaining {
        return Err(Error::Atom("count_exceeds_input"));
    }
    Ok(count.min(PREALLOC_CAP))
}

pub fn decode_term<'a>(
    env: Env<'a>,
    buf: &[u8],
    i: &mut usize,
    limits: &Limits,
    depth: usize,
) -> Result<Term<'a>, Error> {
    let tag = read_u8(buf, i)?;
    match tag {
        0 => { Ok(rustler::types::atom::nil().encode(env)) }
        1 => { Ok(true.encode(env)) }
        2 => { Ok(false.encode(env)) }
        3 => {
            let (negative, mag) = decode_varint_raw(buf, i).map_err(Error::Atom)?;
            let sign = if mag.is_empty() {
                Sign::NoSign
            } else if negative {
                Sign::Minus
            } else {
                Sign::Plus
            };
            Ok(BigInt::from_bytes_be(sign, mag).encode(env))
        }
        5 => {
            let len = decode_varint_gt_zero(buf, i)?;
            let bytes = read_exact(buf, i, len as usize)?;
            let mut ob = OwnedBinary::new(len).ok_or(Error::Atom("alloc_failed"))?;
            ob.as_mut_slice().copy_from_slice(&bytes);
            let bin = ob.release(env);
            Ok(bin.encode(env))
        }
        6 => {
            let count = decode_varint_gt_zero(buf, i)?;
            let cap = account_container(count, buf.len().saturating_sub(*i), limits)?;
            let depth = depth + 1;
            if depth > limits.max_depth {
                return Err(Error::Atom("depth_limit_exceeded"));
            }
            let mut items: Vec<Term> = Vec::with_capacity(cap);
            for _ in 0..count {
                items.push(decode_term(env, buf, i, limits, depth)?);
            }
            Ok(items.encode(env))
        }
        7 => {
            let count = decode_varint_gt_zero(buf, i)?;
            account_container(count, buf.len().saturating_sub(*i), limits)?;
            let depth = depth + 1;
            if depth > limits.max_depth {
                return Err(Error::Atom("depth_limit_exceeded"));
            }
            let mut map = rustler::types::map::map_new(env);

            //Canonical check
            let mut prev_key_bytes: Option<&[u8]> = None;

            for _ in 0..count {
                let k_start = *i;
                let k = decode_term(env, buf, i, limits, depth)?;
                let k_bytes = &buf[k_start..*i];

                if let Some(prev) = prev_key_bytes {
                    if k_bytes <= prev { return Err(Error::Atom("map_not_canonical")); }
                }
                prev_key_bytes = Some(k_bytes);

                let v = decode_term(env, buf, i, limits, depth)?;

                if let Ok(bin) = Binary::from_term(k) {
                    let bytes = bin.as_slice();
                    let atom = if matches!(bytes, b"nil" | b"true" | b"false") {
                        None
                    } else {
                        Atom::try_from_bytes(env, bytes).ok().flatten()
                    };
                    match atom {
                        Some(a) => map = map.map_put(a, v)?,
                        None => map = map.map_put(k, v)?,
                    }
                } else {
                    map = map.map_put(k, v)?;
                }
            }
            Ok(map)
        }
        _ => Err(Error::Atom("unknown_tag")),
    }
}

pub fn decode_term_from_slice<'a>(
    env: Env<'a>,
    buf: &[u8],
    limits: &Limits,
) -> Result<Term<'a>, Error> {
    let mut i = 0;
    let term = decode_term(env, buf, &mut i, limits, 0)?;
    if i != buf.len() { return Err(Error::Atom("trailing_bytes")); }
    Ok(term)
}
