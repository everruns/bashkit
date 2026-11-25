use super::Val;
use crate::builtins::jq::jaq_json::meter;

fn tree(mut leaf: Val) -> Val {
    for _ in 0..13 {
        leaf = [leaf.clone(), leaf].into_iter().collect();
    }
    leaf
}

fn assert_interrupted(operation: impl FnOnce()) {
    let (_guard, _meter) = meter::install(1 << 20, Some(Box::new(|| true)));
    let abort = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
    assert!(abort.is_err_and(|error| {
        error.downcast_ref::<meter::Abort>() == Some(&meter::Abort::Interrupted)
    }));
}

#[test]
fn containment_polls_while_revisiting_shared_descendants() {
    let left = tree(Val::Null);
    let right = tree(Val::Null);
    assert_interrupted(|| {
        std::hint::black_box(left.contains(&right));
    });
}

#[test]
fn byte_conversion_polls_even_when_every_leaf_is_empty() {
    let value = tree(std::iter::empty::<Val>().collect());
    assert_interrupted(|| {
        let _ = std::hint::black_box(value.to_bytes());
    });
}
