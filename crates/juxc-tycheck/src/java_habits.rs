//! Help for Java habits (JUX-DIAGNOSTICS-ADDENDUM, "Java Habits").
//!
//! Jux looks like Java on purpose, so Java habits come with it. Where Jux does
//! a thing differently, the diagnostic a habit raises carries the Jux way
//! instead of only refusing: `equals` is the `==` operator override, a record
//! component is read as `p.x`, and a collection is the Rust std one, so `add`
//! is `push`.
//!
//! Two kinds of habit, kept apart on purpose:
//!
//! - **Language habits** have one answer, because they are language rules:
//!   equality, hashing, ordering and text are operators (§O.2), and a getter
//!   reads a component or property directly.
//! - **Library habits** only ever name a member the receiver really has. The
//!   table below says which Rust std spellings a Java name corresponds to; the
//!   caller says what the receiver's discovered surface contains, and a
//!   candidate it does not contain is never suggested.

/// Java collection spellings and the Rust std ones they correspond to, best
/// first. A candidate is suggested only when the receiver has it.
const LIBRARY_HABITS: &[(&str, &[&str])] = &[
    ("add", &["push", "insert", "push_back"]),
    ("addAll", &["extend", "append"]),
    ("addFirst", &["push_front"]),
    ("addLast", &["push_back", "push"]),
    ("containsKey", &["contains_key"]),
    ("entrySet", &["iter"]),
    ("getFirst", &["first", "front"]),
    ("getLast", &["last", "back"]),
    ("isEmpty", &["is_empty"]),
    ("keySet", &["keys"]),
    ("length", &["len"]),
    ("offer", &["push_back", "push"]),
    ("peek", &["front", "last"]),
    ("poll", &["pop_front"]),
    ("put", &["insert"]),
    ("putIfAbsent", &["entry"]),
    ("removeFirst", &["pop_front"]),
    ("removeLast", &["pop_back", "pop"]),
    ("size", &["len"]),
    ("toArray", &["to_vec"]),
];

/// The help text for calling `method` with `arg_count` arguments on a
/// receiver that lacks it, or `None` when no habit explains the miss.
///
/// `has_member(name)` answers whether the receiver's discovered surface has a
/// method `name`; `readable(name)` whether it has a field, property or record
/// component `name` (a Java getter reads one of those in Jux). The text starts
/// with ` -- `, ready to append to the diagnostic's message.
pub fn member_hint(
    method: &str,
    arg_count: usize,
    has_member: &dyn Fn(&str) -> bool,
    readable: &dyn Fn(&str) -> bool,
) -> Option<String> {
    // Language habits: Java's equality, hashing, ordering and text methods.
    // Jux has none of them; each is an operator the type overrides (§O.2).
    match (method, arg_count) {
        ("equals", 1) => {
            return Some(
                " -- equality is an operator: write `a == b` (override `operator==` to define it)".into(),
            )
        }
        ("hashCode", 0) => {
            return Some(
                " -- hashing is an operator: `x.operator hash()` (override `operator hash` to define it)".into(),
            )
        }
        ("compareTo", 1) => {
            return Some(" -- ordering is an operator: `a <=> b`, or `<` / `>` (override `operator<=>` to define it)".into())
        }
        ("toString", 0) => {
            return Some(
                " -- a value's text is its `operator string` (override it to define it): interpolate it, `$\"${x}\"`, or call `x.operator string()`"
                    .into(),
            )
        }
        _ => {}
    }
    // A getter: `p.x()`, `p.getX()`, `p.isX()` for a readable `x` / `X`.
    if arg_count == 0 {
        if let Some(name) = getter_target(method, readable) {
            return Some(format!(" -- `{name}` is read directly: `x.{name}`, with no call"));
        }
    }
    // Library habits: the Rust std spelling, if the receiver has one.
    let (_, candidates) = LIBRARY_HABITS.iter().find(|(java, _)| *java == method)?;
    let present: Vec<&str> = candidates.iter().copied().filter(|c| has_member(c)).collect();
    match present.as_slice() {
        [] => None,
        [one] => Some(format!(" -- Jux collections are the Rust std ones: use `{one}`")),
        [first, second, ..] => {
            Some(format!(" -- Jux collections are the Rust std ones: use `{first}` or `{second}`"))
        }
    }
}

/// The member a Java-style getter call reads: `x()` reads `x`, `getX()` and
/// `isX()` read `x` or the property `X`. `None` when nothing by those names
/// is readable on the receiver.
fn getter_target(method: &str, readable: &dyn Fn(&str) -> bool) -> Option<String> {
    if readable(method) {
        return Some(method.to_string());
    }
    let rest = method.strip_prefix("get").or_else(|| method.strip_prefix("is"))?;
    let mut chars = rest.chars();
    let first = chars.next()?;
    if !first.is_uppercase() {
        return None;
    }
    // A property is usually PascalCase (`Name`), a field or component
    // camelCase (`name`): try both, property first.
    let pascal = rest.to_string();
    let camel: String = first.to_lowercase().chain(chars).collect();
    [pascal, camel].into_iter().find(|n| readable(n))
}

/// Help for a call through `System.out` / `System.err` (`System.out.println`),
/// which Jux writes as `print(...)`.
pub fn system_out_hint(stream: &str, method: &str) -> Option<&'static str> {
    if !matches!(stream, "out" | "err") {
        return None;
    }
    match method {
        "println" | "print" => Some("Jux prints with `print(...)`"),
        "printf" | "format" => Some("Jux prints with `print(...)`; format with interpolation, `print($\"${x}\")`"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec_like(name: &str) -> bool {
        matches!(name, "push" | "len" | "insert" | "is_empty" | "pop")
    }

    #[test]
    fn library_habits_name_only_what_the_receiver_has() {
        let none = |_: &str| false;
        assert_eq!(
            member_hint("add", 1, &vec_like, &none).as_deref(),
            Some(" -- Jux collections are the Rust std ones: use `push` or `insert`")
        );
        assert_eq!(
            member_hint("size", 0, &vec_like, &none).as_deref(),
            Some(" -- Jux collections are the Rust std ones: use `len`")
        );
        // The receiver has no `pop_front`: say nothing rather than guess.
        assert_eq!(member_hint("poll", 0, &vec_like, &none), None);
        assert_eq!(member_hint("frobnicate", 0, &vec_like, &none), None);
    }

    #[test]
    fn object_methods_are_operators() {
        let none = |_: &str| false;
        assert!(member_hint("equals", 1, &none, &none).unwrap().contains("a == b"));
        assert!(member_hint("hashCode", 0, &none, &none).unwrap().contains("operator hash"));
        assert!(member_hint("compareTo", 1, &none, &none).unwrap().contains("<=>"));
        assert!(member_hint("toString", 0, &none, &none).unwrap().contains("operator string"));
        // Other arities are some other method; no habit applies.
        assert_eq!(member_hint("equals", 2, &none, &none), None);
    }

    #[test]
    fn getters_read_the_member_directly() {
        let none = |_: &str| false;
        let has = |n: &str| matches!(n, "qty" | "Name" | "active");
        assert!(member_hint("qty", 0, &none, &has).unwrap().contains("`x.qty`"));
        assert!(member_hint("getName", 0, &none, &has).unwrap().contains("`x.Name`"));
        assert!(member_hint("isActive", 0, &none, &has).unwrap().contains("`x.active`"));
        assert_eq!(member_hint("getColor", 0, &none, &has), None);
        assert_eq!(member_hint("qty", 1, &none, &has), None);
    }

    #[test]
    fn system_out_is_print() {
        assert!(system_out_hint("out", "println").unwrap().contains("print("));
        assert!(system_out_hint("err", "printf").unwrap().contains("interpolation"));
        assert_eq!(system_out_hint("in", "read"), None);
    }
}
