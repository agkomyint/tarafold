//! Reading `report.json` back. Non-finite numbers are written as `null`; this reader maps a
//! `null` in a number field back to `NaN` (an `Option` field still reads `null` as `None`).

use serde::de::value::{MapDeserializer, SeqDeserializer};
use serde::de::{self, Deserializer, IntoDeserializer, Visitor};
use serde::forward_to_deserialize_any;
use serde_json::Value;

/// A JSON value whose `null`s deserialize as `NaN` when a float is expected.
pub(crate) struct Lenient(pub Value);

impl<'de> IntoDeserializer<'de, serde_json::Error> for Lenient {
    type Deserializer = Self;
    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'de> Deserializer<'de> for Lenient {
    type Error = serde_json::Error;

    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::Null => v.visit_unit(),
            Value::Bool(b) => v.visit_bool(b),
            Value::Number(n) => {
                if let Some(u) = n.as_u64() {
                    v.visit_u64(u)
                } else if let Some(i) = n.as_i64() {
                    v.visit_i64(i)
                } else {
                    v.visit_f64(n.as_f64().unwrap_or(f64::NAN))
                }
            }
            Value::String(s) => v.visit_string(s),
            Value::Array(a) => {
                let mut seq = SeqDeserializer::new(a.into_iter().map(Lenient));
                let out = v.visit_seq(&mut seq)?;
                seq.end()?;
                Ok(out)
            }
            Value::Object(m) => {
                let mut map = MapDeserializer::new(m.into_iter().map(|(k, x)| (k, Lenient(x))));
                let out = v.visit_map(&mut map)?;
                map.end()?;
                Ok(out)
            }
        }
    }

    fn deserialize_f64<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::Null => v.visit_f64(f64::NAN),
            other => Lenient(other).deserialize_any(v),
        }
    }

    fn deserialize_f32<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        self.deserialize_f64(v)
    }

    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::Null => v.visit_none(),
            other => v.visit_some(Lenient(other)),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        v: V,
    ) -> Result<V::Value, Self::Error> {
        v.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, Self::Error> {
        match self.0 {
            Value::String(s) => v.visit_enum(s.into_deserializer()),
            Value::Object(m) if m.len() == 1 => {
                let Some((k, val)) = m.into_iter().next() else {
                    return Err(de::Error::custom("empty enum object"));
                };
                v.visit_enum(EnumAccess {
                    variant: k,
                    value: Lenient(val),
                })
            }
            other => other.deserialize_enum(name, variants, v),
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 char str string bytes byte_buf unit
        unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

struct EnumAccess {
    variant: String,
    value: Lenient,
}

impl<'de> de::EnumAccess<'de> for EnumAccess {
    type Error = serde_json::Error;
    type Variant = Lenient;

    fn variant_seed<S: de::DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Lenient), Self::Error> {
        let v = seed.deserialize(self.variant.into_deserializer())?;
        Ok((v, self.value))
    }
}

impl<'de> de::VariantAccess<'de> for Lenient {
    type Error = serde_json::Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<S: de::DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<S::Value, Self::Error> {
        seed.deserialize(self)
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, v: V) -> Result<V::Value, Self::Error> {
        self.deserialize_any(v)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_any(v)
    }
}
