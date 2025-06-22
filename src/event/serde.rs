use std::{any::type_name, fmt, marker::PhantomData, str::FromStr};

use serde::{Deserialize, Serialize, de};

use crate::event::{Abs, Key, Led, Misc, Rel, Sound, Switch};

struct NamedOrRawVisitor<T: FromStr, F: Fn(u16) -> T> {
    from_raw: F,
    _p: PhantomData<T>,
}

impl<T: FromStr, F: Fn(u16) -> T> NamedOrRawVisitor<T, F> {
    fn new(from_raw: F) -> Self {
        Self {
            from_raw,
            _p: PhantomData,
        }
    }
}

impl<'de, T: FromStr, F: Fn(u16) -> T> de::Visitor<'de> for NamedOrRawVisitor<T, F> {
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("named variant or raw code")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        T::from_str(v).map_err(|_| {
            E::custom(format!(
                "unknown variant '{v}' for type '{}'",
                type_name::<T>()
            ))
        })
    }

    fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok((self.from_raw)(v))
    }

    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let v: u16 = match v.try_into() {
            Ok(v) => v,
            Err(_) => {
                return Err(E::invalid_value(
                    de::Unexpected::Unsigned(v.into()),
                    &"unsigned 16-bit value",
                ));
            }
        };
        self.visit_u16(v)
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let v: u16 = match v.try_into() {
            Ok(v) => v,
            Err(_) => {
                return Err(E::invalid_value(
                    de::Unexpected::Unsigned(v.into()),
                    &"unsigned 16-bit value",
                ));
            }
        };
        self.visit_u16(v)
    }
}

macro_rules! serde_impls {
    ( $($t:ident),* ) => {
        $(
            /// Deserialization from a raw 16-bit code or a named constant.
            impl<'a> Deserialize<'a> for $t {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where
                    D: serde::Deserializer<'a>,
                {
                    if deserializer.is_human_readable() {
                        // For human-readable formats, codes can be provided either as names
                        // (`KEY_F1`, `ABS_X`, ...) or as raw `u16` values.
                        // We assume that all human-readable formats are also self-describing and
                        // thus support `deserialize_any`.
                        deserializer.deserialize_any(NamedOrRawVisitor::new(<$t>::from_raw))
                    } else {
                        // Binary formats always use the raw u16 code.
                        let raw = u16::deserialize(deserializer)?;
                        Ok(<$t>::from_raw(raw))
                    }
                }
            }

            impl Serialize for $t {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: serde::Serializer,
                {
                    if serializer.is_human_readable() {
                        // For human-readable formats, we prefer the textual name if there is one.
                        // If not, we use the raw u16 code.
                        // Like above, we assume human readable formats are also self-describing.
                        match self.name() {
                            Some(name) => serializer.collect_str(&name),
                            None => self.raw().serialize(serializer),
                        }
                    } else {
                        // Binary formats always use the raw u16 code.
                        self.raw().serialize(serializer)
                    }
                }
            }
        )*
    };
}

serde_impls!(Abs, Key, Rel, Misc, Led, Switch, Sound);

#[cfg(test)]
mod tests {
    use csv::{ReaderBuilder, WriterBuilder};

    use super::*;

    #[test]
    fn csv() {
        // CSV is human-readable, but not completely self-describing, but the `csv` crate infers
        // the types correctly, so things work out for us.

        let mut out = Vec::new();
        let mut w = WriterBuilder::new().from_writer(&mut out);
        w.serialize(Key::KEY_F12).unwrap();
        w.serialize(Key::from_raw(0xffff)).unwrap();
        w.flush().unwrap();
        drop(w);

        let s = String::from_utf8(out).unwrap();
        assert_eq!(s, "KEY_F12\n65535\n");

        let mut r = ReaderBuilder::new()
            .has_headers(false)
            .from_reader(s.as_bytes());
        let mut iter = r.deserialize::<Key>();
        let key = iter.next().unwrap().unwrap();
        assert_eq!(key, Key::KEY_F12);
        let key = iter.next().unwrap().unwrap();
        assert_eq!(key, Key::from_raw(0xffff));
        assert!(iter.next().is_none());
    }

    #[test]
    fn postcard() {
        let b = postcard::to_allocvec(&Key::KEY_F12).unwrap();
        assert_eq!(postcard::from_bytes::<Key>(&b).unwrap(), Key::KEY_F12);

        let b = postcard::to_allocvec(&Key::from_raw(0xffff)).unwrap();
        assert_eq!(
            postcard::from_bytes::<Key>(&b).unwrap(),
            Key::from_raw(0xffff)
        );
    }
}
