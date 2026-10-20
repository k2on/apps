//! The nearest thing outside each row of `spec/AUTHORING.md`'s table is
//! a compile error naming the construct (`tests/ui/*.rs`, its words in
//! the `.stderr` beside it). Falsified by lowering `while` as a block: the
//! loop was taken, and `a_loop.rs` compiled.

#[test]
fn outside_the_subset_is_refused_by_name() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
