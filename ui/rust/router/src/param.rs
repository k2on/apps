//! What a route carries in its path or its query, typed: a route file's
//! `[id].rs` names a field of its struct, and the field's type says what
//! the segment reads as (`String`, a number, a flag). A query field is the
//! same, or an `Option` of one, which is left out when it is `None`.

/// A value a route carries as one path segment or one query value: written
/// as its text, read back from it.
pub trait Param: Sized {
    /// Its text. Empty is nothing: a query leaves it out, and a path that
    /// would need it is not written with it.
    fn param(&self) -> String;
    /// The value `text` spells (never empty), or `None` when it spells
    /// none: the route does not match.
    fn of_param(text: &str) -> Option<Self>;
}

impl Param for String {
    fn param(&self) -> String {
        self.clone()
    }
    fn of_param(text: &str) -> Option<String> {
        Some(text.to_string())
    }
}

/// A flag is `1`, and not written when it is down (ui/docs/app.md,
/// "Addresses").
impl Param for bool {
    fn param(&self) -> String {
        match self {
            true => "1".into(),
            false => String::new(),
        }
    }
    fn of_param(text: &str) -> Option<bool> {
        match text {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        }
    }
}

macro_rules! numbers {
    ($($t:ty),*) => {$(
        impl Param for $t {
            fn param(&self) -> String {
                self.to_string()
            }
            fn of_param(text: &str) -> Option<$t> {
                text.parse().ok()
            }
        }
    )*};
}

numbers!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

#[cfg(test)]
mod tests {
    use super::*;

    /// Each kind reads back as it was written, and a text that spells none
    /// of it is none. Falsified by reading a number leniently (`"12x"` as 12).
    #[test]
    fn each_kind_reads_as_it_was_written() {
        assert_eq!(String::of_param(&"a b".to_string().param()), Some("a b".into()));
        assert_eq!(u32::of_param(&42u32.param()), Some(42));
        assert_eq!(i64::of_param("-3"), Some(-3));
        assert_eq!(u32::of_param("12x"), None);
        assert_eq!(true.param(), "1");
        assert_eq!(false.param(), "", "a flag down is not written");
        assert_eq!(bool::of_param("1"), Some(true));
        assert_eq!(bool::of_param("yes"), None);
    }
}
