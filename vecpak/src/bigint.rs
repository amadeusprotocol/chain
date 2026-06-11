use num_bigint::{BigInt, Sign};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

pub(crate) const BIGVARINT_TOKEN: &str = "$__vecpak_bigvarint";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigVarInt(pub BigInt);

impl From<BigInt> for BigVarInt {
    fn from(b: BigInt) -> Self {
        BigVarInt(b)
    }
}
impl From<BigVarInt> for BigInt {
    fn from(b: BigVarInt) -> Self {
        b.0
    }
}

pub(crate) fn pack(b: &BigInt) -> Vec<u8> {
    let (sign, mag) = b.to_bytes_be();
    let mut out = Vec::with_capacity(mag.len() + 1);
    match sign {
        Sign::NoSign => out.push(0),
        Sign::Plus => {
            out.push(0);
            out.extend_from_slice(&mag);
        }
        Sign::Minus => {
            out.push(1);
            out.extend_from_slice(&mag);
        }
    }
    out
}

pub(crate) fn unpack(packed: &[u8]) -> BigInt {
    if packed.len() <= 1 {
        return BigInt::from(0);
    }
    let sign = if packed[0] == 1 { Sign::Minus } else { Sign::Plus };
    BigInt::from_bytes_be(sign, &packed[1..])
}

struct Packed<'a>(&'a [u8]);

impl Serialize for Packed<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(self.0)
    }
}

impl Serialize for BigVarInt {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let packed = pack(&self.0);
        s.serialize_newtype_struct(BIGVARINT_TOKEN, &Packed(&packed))
    }
}

struct BigVarIntVisitor;

impl<'de> de::Visitor<'de> for BigVarIntVisitor {
    type Value = BigVarInt;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a vecpak big varint")
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
        Ok(BigVarInt(unpack(v)))
    }

    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        Ok(BigVarInt(unpack(&v)))
    }
}

impl<'de> Deserialize<'de> for BigVarInt {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_newtype_struct(BIGVARINT_TOKEN, BigVarIntVisitor)
    }
}
