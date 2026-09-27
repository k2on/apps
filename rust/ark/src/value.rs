//! §1 Values, as `Ark.Value` defines them.
//!
//! The eight run-time values three generated codebases have to agree about
//! to the byte, and the one total order over them. There are no floats. An
//! enum variant is a [`Value::Text`] at run time.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

/// The name of a table, as declared in the schema.
pub type TableName = String;

/// The name of a field of a struct, or a column of a row.
pub type FieldName = String;

/// Sixteen bytes. The table an id names is a static type, never a run-time
/// tag (`Ark.Value.IdBytes`).
pub type Id = [u8; 16];

/// A run-time value (`Ark.Value.Value`).
///
/// `Null` exists only as the absent case of an `Option` type. `Struct` is a
/// named record whose field names are its identity; a row of a table is a
/// `Struct` of every column, never partial.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Text(String),
    Bytes(Vec<u8>),
    Id(Id),
    List(Vec<Value>),
    Struct(BTreeMap<FieldName, Value>),
}

/// The rank of a value's type in the total order:
/// `Null < Bool < Int < Text < Bytes < Id < List < Struct` (`Ark.Value.rank`).
pub fn rank(v: &Value) -> u8 {
    match v {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Int(_) => 2,
        Value::Text(_) => 3,
        Value::Bytes(_) => 4,
        Value::Id(_) => 5,
        Value::List(_) => 6,
        Value::Struct(_) => 7,
    }
}

/// §1.2 The total order (`Ark.Value.compareValue`).
///
/// Values of different types order by [`rank`]; within a type: ints
/// numerically; text by Unicode code point, which is UTF-8 byte order and is
/// what comparing the bytes does; bytes and ids lexicographically; lists
/// lexicographically with a prefix first; structs as the association list
/// sorted by field name, names before values.
pub fn compare_value(a: &Value, b: &Value) -> Ordering {
    let (ra, rb) = (rank(a), rank(b));
    if ra != rb {
        return ra.cmp(&rb);
    }
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        // UTF-8 byte order is code point order.
        (Value::Text(x), Value::Text(y)) => x.as_bytes().cmp(y.as_bytes()),
        (Value::Bytes(x), Value::Bytes(y)) => x.cmp(y),
        (Value::Id(x), Value::Id(y)) => x.cmp(y),
        (Value::List(xs), Value::List(ys)) => {
            for (x, y) in xs.iter().zip(ys.iter()) {
                let o = compare_value(x, y);
                if o != Ordering::Equal {
                    return o;
                }
            }
            xs.len().cmp(&ys.len())
        }
        (Value::Struct(xs), Value::Struct(ys)) => {
            // Both maps iterate in byte order of the key, which is the
            // code point order the spec sorts the association list by.
            for ((k, v), (k2, v2)) in xs.iter().zip(ys.iter()) {
                let o = k.as_bytes().cmp(k2.as_bytes());
                if o != Ordering::Equal {
                    return o;
                }
                let o = compare_value(v, v2);
                if o != Ordering::Equal {
                    return o;
                }
            }
            xs.len().cmp(&ys.len())
        }
        _ => unreachable!("compare_value: rank mismatch is impossible"),
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_value(self, other)
    }
}

/// Whether a value is `Null` (`Ark.Value.isNull`).
pub fn is_null(v: &Value) -> bool {
    matches!(v, Value::Null)
}

// The constructors and accessors GENERATED.md names, in snake case. The
// accessors fail fatally on a type mismatch: a verified module never
// mismatches, so a mismatch is a bug in a generator and not a fault.
impl Value {
    /// `Value.null()`.
    pub fn null() -> Value {
        Value::Null
    }

    /// `Value.bool(b)`.
    pub fn bool(b: bool) -> Value {
        Value::Bool(b)
    }

    /// `Value.int(i64)`.
    pub fn int(n: i64) -> Value {
        Value::Int(n)
    }

    /// `Value.text(s)`.
    pub fn text<S: Into<String>>(s: S) -> Value {
        Value::Text(s.into())
    }

    /// A byte string from its bytes.
    pub fn bytes<B: Into<Vec<u8>>>(b: B) -> Value {
        Value::Bytes(b.into())
    }

    /// `Value.bytesHex("0a0b")`: a byte string from lowercase or uppercase
    /// hex. Panics on anything that is not hex of even length, which in
    /// generated code is a literal and therefore a bug.
    pub fn bytes_hex(hex: &str) -> Value {
        Value::Bytes(decode_hex(hex).unwrap_or_else(|| panic!("Value::bytes_hex: not hex: {hex:?}")))
    }

    /// An id from its sixteen bytes.
    pub fn id(id: Id) -> Value {
        Value::Id(id)
    }

    /// `Value.idHex("32 hex digits")`: an id from exactly 32 hex digits
    /// (dashes are accepted and ignored, so an 8-4-4-4-12 spelling works
    /// too). Panics otherwise.
    pub fn id_hex(hex: &str) -> Value {
        let plain: String = hex.chars().filter(|c| *c != '-').collect();
        let bytes = decode_hex(&plain).unwrap_or_else(|| panic!("Value::id_hex: not hex: {hex:?}"));
        let id: Id = bytes
            .as_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("Value::id_hex: an id is 32 hex digits, not {hex:?}"));
        Value::Id(id)
    }

    /// An option, flat: `Some(v)` is `v` and `None` is `Null` (the verifier
    /// forbids an option of an option, which is what makes this
    /// unambiguous).
    pub fn opt(v: Option<Value>) -> Value {
        v.unwrap_or(Value::Null)
    }

    /// `Value.list([v…])`.
    pub fn list(items: Vec<Value>) -> Value {
        Value::List(items)
    }

    /// `Value.record([("k", v)…])`: a struct from its fields. A repeated
    /// name keeps the last value.
    pub fn record<K: Into<String>>(fields: Vec<(K, Value)>) -> Value {
        Value::Struct(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// `v.isNull()`.
    pub fn is_null(&self) -> bool {
        is_null(self)
    }

    /// `v.asBool()`; fatal on anything else.
    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            other => panic!("as_bool: expected Bool, got {other:?} (a bug: a verified module never mismatches)"),
        }
    }

    /// `v.asInt()`; fatal on anything else.
    pub fn as_int(&self) -> i64 {
        match self {
            Value::Int(n) => *n,
            other => panic!("as_int: expected Int, got {other:?} (a bug: a verified module never mismatches)"),
        }
    }

    /// `v.asText()`; fatal on anything else.
    pub fn as_text(&self) -> &str {
        match self {
            Value::Text(t) => t,
            other => panic!("as_text: expected Text, got {other:?} (a bug: a verified module never mismatches)"),
        }
    }

    /// The bytes of a `Bytes`; fatal on anything else.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Value::Bytes(b) => b,
            other => panic!("as_bytes: expected Bytes, got {other:?} (a bug: a verified module never mismatches)"),
        }
    }

    /// The sixteen bytes of an `Id`; fatal on anything else.
    pub fn as_id(&self) -> Id {
        match self {
            Value::Id(i) => *i,
            other => panic!("as_id: expected Id, got {other:?} (a bug: a verified module never mismatches)"),
        }
    }

    /// `v.asList()`: the elements, owned, so that `for x in v.as_list()`
    /// binds a `Value`; fatal on anything else.
    pub fn as_list(&self) -> Vec<Value> {
        match self {
            Value::List(xs) => xs.clone(),
            other => panic!("as_list: expected List, got {other:?} (a bug: a verified module never mismatches)"),
        }
    }

    /// The fields of a `Struct`, by reference; fatal on anything else.
    pub fn as_struct(&self) -> &BTreeMap<FieldName, Value> {
        match self {
            Value::Struct(m) => m,
            other => panic!("as_struct: expected Struct, got {other:?} (a bug: a verified module never mismatches)"),
        }
    }

    /// `v.field("k")`: the field's value, owned; fatal when `v` is not a
    /// struct or has no such field.
    pub fn field(&self, name: &str) -> Value {
        match self {
            Value::Struct(m) => m
                .get(name)
                .cloned()
                .unwrap_or_else(|| panic!("field: no field {name:?} in {self:?} (a bug: a verified module never mismatches)")),
            other => panic!("field: expected Struct, got {other:?} (a bug: a verified module never mismatches)"),
        }
    }
}

/// The text of a `Value::Text`, for a generated `Fault::refuse(Value::text(…))`:
/// the emitter spells a refusal's reason as a value expression, and
/// `Fault::refuse` takes anything `Into<String>`. Fatal on anything but a
/// text, as `as_text` is.
impl From<Value> for String {
    fn from(v: Value) -> String {
        match v {
            Value::Text(t) => t,
            other => panic!("String::from: expected Text, got {other:?} (a bug: a verified module never mismatches)"),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Lowercase hex, two digits a byte (what `Ark.Std.hexText` computes).
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

/// Bytes from hex in either case; `None` unless every character is a hex
/// digit and there is an even number of them.
pub fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = s.bytes().map(hex_val).collect::<Option<_>>()?;
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    Some(digits.chunks(2).map(|p| (p[0] << 4) | p[1]).collect())
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_orders_by_code_point() {
        // U+FF5E is after U+1F3B5 in UTF-16 units and before it in code points.
        assert_eq!(compare_value(&Value::text("\u{FF5E}"), &Value::text("\u{1F3B5}")), Ordering::Less);
        assert_ne!(compare_value(&Value::text("e\u{0301}"), &Value::text("é")), Ordering::Equal);
    }

    #[test]
    fn a_prefix_list_is_first_and_ranks_separate_types() {
        assert_eq!(compare_value(&Value::list(vec![]), &Value::list(vec![Value::int(1)])), Ordering::Less);
        assert_eq!(compare_value(&Value::Null, &Value::bool(false)), Ordering::Less);
        assert_eq!(compare_value(&Value::bytes(vec![]), &Value::list(vec![])), Ordering::Less);
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex(&[0, 255, 16]), "00ff10");
        assert_eq!(decode_hex("00FF10"), Some(vec![0, 255, 16]));
        assert_eq!(decode_hex("0"), None);
        assert_eq!(
            Value::id_hex("00000000-0000-0000-0000-000000000001"),
            Value::Id([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])
        );
    }

    #[test]
    fn a_text_value_is_a_string_for_refuse() {
        let f = crate::fault::Fault::refuse(Value::text("a playlist needs a name"));
        assert_eq!(f, crate::fault::Fault::Refuse("a playlist needs a name".into()));
        assert_eq!(String::from(Value::text("x")), "x");
    }
}
