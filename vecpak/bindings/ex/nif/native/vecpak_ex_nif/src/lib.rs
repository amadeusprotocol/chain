use rustler::types::{Binary, OwnedBinary};
use rustler::{
    Encoder, Env, Error, Term
};
use vecpak_ex::{ encode_term, decode_term_from_slice, parse_limits };

#[rustler::nif(schedule = "DirtyCpu")]
fn encode<'a>(env: Env<'a>, map: Term<'a>) -> Result<Term<'a>, Error> {
    let mut buf = Vec::with_capacity(1024);
    encode_term(env, &mut buf, map, 0)?;

    let mut ob = OwnedBinary::new(buf.len()).ok_or_else(|| Error::Term(Box::new("alloc failed")))?;
    ob.as_mut_slice().copy_from_slice(&buf);

    Ok(Binary::from_owned(ob, env).encode(env))
}

#[rustler::nif(schedule = "DirtyCpu", name = "decode")]
fn decode_with_opts<'a>(env: Env<'a>, bin: Binary, opts: Term<'a>) -> Result<Term<'a>, Error> {
    let limits = parse_limits(opts)?;
    let term = decode_term_from_slice(env, bin.as_slice(), &limits)?;
    Ok(term.encode(env))
}

rustler::init!("Elixir.VecPak");
