use crate::{
    decode_varint, encode_varint,
    error::{Error, Result},
};
use serde::{ser, Serialize};

pub(crate) fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

fn variant_wire_name(variant: &'static str) -> Result<String> {
    let snake = to_snake_case(variant);
    if crate::de::to_pascal_case(&snake) != variant {
        return Err(Error::Message(format!(
            "enum variant name '{variant}' is not round-trip-safe through the \
             snake_case wire convention (use idiomatic CamelCase or #[serde(rename)])"
        )));
    }
    Ok(snake)
}

fn map_variant_wire_name(variant: &'static str) -> Result<String> {
    let snake = variant_wire_name(variant)?;
    if snake == "op" {
        return Err(Error::Message(
            "enum variant name 'op' conflicts with the variant tag".into(),
        ));
    }
    Ok(snake)
}

pub struct Serializer {
    output: Vec<u8>,
    depth: usize,
    limits: crate::Limits,
}

pub fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    to_vec_with_limits(value, &crate::Limits::default())
}

pub fn to_vec_with_limits<T: Serialize>(value: &T, limits: &crate::Limits) -> Result<Vec<u8>> {
    let mut serializer = Serializer {
        output: Vec::new(),
        depth: 0,
        limits: *limits,
    };
    value.serialize(&mut serializer)?;
    Ok(serializer.output)
}

impl Serializer {
    fn child(&self) -> Serializer {
        Serializer {
            output: Vec::new(),
            depth: self.depth,
            limits: self.limits,
        }
    }
    #[inline]
    fn enter(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > self.limits.max_depth {
            return Err(Error::LimitExceeded("depth_limit_exceeded"));
        }
        Ok(())
    }
    #[inline]
    fn leave(&mut self) {
        self.depth -= 1;
    }
    #[inline]
    fn check_len(&self, len: usize) -> Result<()> {
        if len > self.limits.max_container_len {
            return Err(Error::LimitExceeded("container_too_large"));
        }
        Ok(())
    }
}

impl<'a> ser::Serializer for &'a mut Serializer {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = SeqSerializer<'a>;
    type SerializeTuple = SeqSerializer<'a>;
    type SerializeTupleStruct = SeqSerializer<'a>;
    type SerializeTupleVariant = TupleVariantSerializer<'a>;
    type SerializeMap = MapSerializer<'a>;
    type SerializeStruct = MapSerializer<'a>;
    type SerializeStructVariant = StructVariantSerializer<'a>;

    fn serialize_bool(self, v: bool) -> Result<()> {
        self.output.push(if v { 1 } else { 2 });
        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<()> {
        self.serialize_i128(v as i128)
    }
    fn serialize_i16(self, v: i16) -> Result<()> {
        self.serialize_i128(v as i128)
    }
    fn serialize_i32(self, v: i32) -> Result<()> {
        self.serialize_i128(v as i128)
    }
    fn serialize_i64(self, v: i64) -> Result<()> {
        self.serialize_i128(v as i128)
    }
    fn serialize_i128(self, v: i128) -> Result<()> {
        self.output.push(3);
        encode_varint(&mut self.output, v);
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Result<()> {
        self.serialize_i128(v as i128)
    }
    fn serialize_u16(self, v: u16) -> Result<()> {
        self.serialize_i128(v as i128)
    }
    fn serialize_u32(self, v: u32) -> Result<()> {
        self.serialize_i128(v as i128)
    }
    fn serialize_u64(self, v: u64) -> Result<()> {
        self.serialize_i128(v as i128)
    }
    fn serialize_u128(self, v: u128) -> Result<()> {
        if v > i128::MAX as u128 {
            return Err(Error::Message("u128 too large".into()));
        }
        self.serialize_i128(v as i128)
    }

    fn serialize_f32(self, _v: f32) -> Result<()> {
        Err(Error::Message("floats not supported".into()))
    }
    fn serialize_f64(self, _v: f64) -> Result<()> {
        Err(Error::Message("floats not supported".into()))
    }

    fn serialize_char(self, v: char) -> Result<()> {
        self.serialize_str(&v.to_string())
    }

    fn serialize_str(self, v: &str) -> Result<()> {
        self.output.push(5);
        encode_varint(&mut self.output, v.len() as i128);
        self.output.extend_from_slice(v.as_bytes());
        Ok(())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<()> {
        self.output.push(5);
        encode_varint(&mut self.output, v.len() as i128);
        self.output.extend_from_slice(v);
        Ok(())
    }

    fn serialize_none(self) -> Result<()> {
        self.output.push(0);
        Ok(())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<()> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<()> {
        self.output.push(0);
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<()> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
    ) -> Result<()> {
        let snake = variant_wire_name(variant)?;
        self.serialize_str(&snake)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<()> {
        if name == crate::bigint::BIGVARINT_TOKEN {
            let mut scratch = self.child();
            value.serialize(&mut scratch)?;
            let packed = binary_payload(&scratch.output)?;
            let negative = packed.first() == Some(&1);
            self.output.push(3);
            crate::encode_varint_bytes(&mut self.output, negative, &packed[1..])
                .map_err(|e| Error::Message(e.into()))?;
            return Ok(());
        }
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<()> {
        let snake = map_variant_wire_name(variant)?;
        self.enter()?;
        let mut inner_serializer = self.child();
        value.serialize(&mut inner_serializer)?;
        let inner_bytes = inner_serializer.output;

        self.output.push(7);
        encode_varint(&mut self.output, 1);
        self.serialize_str(&snake)?;
        self.output.extend_from_slice(&inner_bytes);
        self.leave();
        Ok(())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq> {
        self.enter()?;
        Ok(SeqSerializer {
            ser: self,
            buf: Vec::new(),
            count: 0,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        let snake = map_variant_wire_name(variant)?;
        self.check_len(len)?;
        self.enter()?;
        self.enter()?;
        Ok(TupleVariantSerializer {
            ser: self,
            snake,
            buf: Vec::new(),
            count: 0,
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        self.enter()?;
        Ok(MapSerializer::new(self))
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        self.enter()?;
        Ok(MapSerializer::new(self))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        let snake = map_variant_wire_name(variant)?;
        let mut variant_bytes = Vec::new();
        variant_bytes.push(5);
        encode_varint(&mut variant_bytes, snake.len() as i128);
        variant_bytes.extend_from_slice(snake.as_bytes());
        self.enter()?;
        Ok(StructVariantSerializer {
            ser: self,
            variant_bytes,
            entries: Vec::new(),
        })
    }
}

pub struct SeqSerializer<'a> {
    ser: &'a mut Serializer,
    buf: Vec<u8>,
    count: usize,
}

impl SeqSerializer<'_> {
    fn push<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        let mut child = self.ser.child();
        value.serialize(&mut child)?;
        self.buf.extend_from_slice(&child.output);
        self.count += 1;
        Ok(())
    }
    fn finish(self) -> Result<()> {
        self.ser.check_len(self.count)?;
        self.ser.output.push(6);
        encode_varint(&mut self.ser.output, self.count as i128);
        self.ser.output.extend_from_slice(&self.buf);
        self.ser.leave();
        Ok(())
    }
}

impl ser::SerializeSeq for SeqSerializer<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        self.push(value)
    }
    fn end(self) -> Result<()> {
        self.finish()
    }
}

impl ser::SerializeTuple for SeqSerializer<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        self.push(value)
    }
    fn end(self) -> Result<()> {
        self.finish()
    }
}

impl ser::SerializeTupleStruct for SeqSerializer<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        self.push(value)
    }
    fn end(self) -> Result<()> {
        self.finish()
    }
}

pub struct TupleVariantSerializer<'a> {
    ser: &'a mut Serializer,
    snake: String,
    buf: Vec<u8>,
    count: usize,
}

impl ser::SerializeTupleVariant for TupleVariantSerializer<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        let mut child = self.ser.child();
        value.serialize(&mut child)?;
        self.buf.extend_from_slice(&child.output);
        self.count += 1;
        Ok(())
    }
    fn end(self) -> Result<()> {
        self.ser.output.push(7);
        encode_varint(&mut self.ser.output, 1);
        self.ser.output.push(5);
        encode_varint(&mut self.ser.output, self.snake.len() as i128);
        self.ser.output.extend_from_slice(self.snake.as_bytes());
        self.ser.output.push(6);
        encode_varint(&mut self.ser.output, self.count as i128);
        self.ser.output.extend_from_slice(&self.buf);
        self.ser.leave();
        self.ser.leave();
        Ok(())
    }
}

pub struct StructVariantSerializer<'a> {
    ser: &'a mut Serializer,
    variant_bytes: Vec<u8>,
    entries: Vec<(Vec<u8>, Vec<u8>)>,
}

impl<'a> ser::SerializeStructVariant for StructVariantSerializer<'a> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        if key == "op" {
            return Err(Error::Message(
                "field 'op' conflicts with enum variant tag".into(),
            ));
        }
        let mut key_serializer = self.ser.child();
        key.serialize(&mut key_serializer)?;
        let mut value_serializer = self.ser.child();
        value.serialize(&mut value_serializer)?;
        self.entries
            .push((key_serializer.output, value_serializer.output));
        Ok(())
    }

    fn end(self) -> Result<()> {
        let mut entries = self.entries;
        let mut op_key = vec![5];
        encode_varint(&mut op_key, 2);
        op_key.extend_from_slice(b"op");
        entries.push((op_key, self.variant_bytes));
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        if entries.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(Error::Message("duplicate_map_key".into()));
        }
        self.ser.check_len(entries.len())?;
        self.ser.output.push(7);
        encode_varint(&mut self.ser.output, entries.len() as i128);
        for (key_bytes, value_bytes) in entries {
            self.ser.output.extend_from_slice(&key_bytes);
            self.ser.output.extend_from_slice(&value_bytes);
        }
        self.ser.leave();
        Ok(())
    }
}

pub struct MapSerializer<'a> {
    ser: &'a mut Serializer,
    entries: Vec<(Vec<u8>, Vec<u8>)>,
}

impl<'a> MapSerializer<'a> {
    fn new(ser: &'a mut Serializer) -> Self {
        MapSerializer {
            ser,
            entries: Vec::new(),
        }
    }

    fn end_map(self) -> Result<()> {
        let mut entries = self.entries;
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        if entries.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(Error::Message("duplicate_map_key".into()));
        }
        self.ser.check_len(entries.len())?;
        self.ser.output.push(7);
        encode_varint(&mut self.ser.output, entries.len() as i128);
        for (key_bytes, value_bytes) in entries {
            self.ser.output.extend_from_slice(&key_bytes);
            self.ser.output.extend_from_slice(&value_bytes);
        }
        self.ser.leave();
        Ok(())
    }
}

impl<'a> ser::SerializeMap for MapSerializer<'a> {
    type Ok = ();
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<()> {
        let mut key_serializer = self.ser.child();
        key.serialize(&mut key_serializer)?;
        self.entries.push((key_serializer.output, Vec::new()));
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        let mut value_serializer = self.ser.child();
        value.serialize(&mut value_serializer)?;
        self.entries.last_mut().unwrap().1 = value_serializer.output;
        Ok(())
    }

    fn end(self) -> Result<()> {
        self.end_map()
    }
}

impl<'a> ser::SerializeStruct for MapSerializer<'a> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        ser::SerializeMap::serialize_key(self, key)?;
        ser::SerializeMap::serialize_value(self, value)
    }

    fn end(self) -> Result<()> {
        self.end_map()
    }
}

fn binary_payload(buf: &[u8]) -> Result<&[u8]> {
    if buf.first() != Some(&5) {
        return Err(Error::Message("bigvarint: expected packed bytes".into()));
    }
    let (len, read) = decode_varint_with_len(&buf[1..]).map_err(|e| Error::Message(e.into()))?;
    let start = 1 + read;
    let end = start + len as usize;
    if end > buf.len() {
        return Err(Error::Message("bigvarint: short payload".into()));
    }
    let payload = &buf[start..end];
    if payload.is_empty() {
        return Err(Error::Message("bigvarint: empty packed payload".into()));
    }
    Ok(payload)
}

/// Convenience function to decode a varint and return the value and its length
#[inline]
pub fn decode_varint_with_len(buf: &[u8]) -> std::result::Result<(i128, usize), &'static str> {
    let mut i = 0;
    let val = decode_varint(buf, &mut i)?;
    Ok((val, i))
}
