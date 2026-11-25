use super::*;

#[test]
fn refused_attachment_leaves_the_container_valid() {
    let (_guard, meter) = meter::install(1 << 20, None);
    let mut nested = Val::Null;
    for _ in 0..meter::MAX_VALUE_DEPTH {
        nested = std::iter::once(nested).collect();
    }
    let mut array = Container::<Vec<Val>>::default();
    let abort = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = array.push(nested);
    }));
    assert!(abort.is_err());
    assert!(meter.tripped());
    assert!(array.is_empty());
}

#[test]
fn failed_cow_does_not_copy_or_change_the_shared_body() {
    let (_guard, meter) = meter::install(64, None);
    let mut array = Rc::new(array(vec![Val::Null; 4]));
    let shared = array.clone();
    assert_eq!(meter::headroom(), 0);
    let abort = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = Rc::make_mut(&mut array).set(0, Val::Bool(true));
    }));
    assert!(abort.is_err());
    assert!(meter.tripped());
    assert!(Rc::ptr_eq(&array, &shared));
    assert!(matches!(shared[0], Val::Null));
}

#[test]
fn failed_object_growth_does_not_reserve_storage() {
    let key = Val::utf8_str("key");
    // A first IndexMap allocation has more than one entry of capacity.
    let (_guard, meter) = meter::install(128, None);
    let mut object = Container::<Map>::default();
    // Once tripped, constructing an error string also aborts the run.
    let abort = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = object.insert(key, Val::Null);
    }));
    assert!(abort.is_err());
    assert!(meter.tripped());
    assert_eq!(object.capacity(), 0);
    assert!(object.is_empty());
}

#[test]
fn removal_invalidates_depth_and_drop_refunds_storage() {
    let (_guard, _meter) = meter::install(4096, None);
    let initial = meter::headroom();
    {
        let nested: Val = std::iter::once(Val::Null).collect();
        let mut values = array([nested]);
        assert_eq!(values.depth(ValueDepth::value_depth), 2);
        values.remove(0);
        assert_eq!(values.depth(ValueDepth::value_depth), 1);
        assert!(meter::headroom() < initial);
    }
    assert_eq!(meter::headroom(), initial);
}

#[test]
fn owning_iterator_keeps_storage_charged_until_drop() {
    let (_guard, _meter) = meter::install(4096, None);
    let initial = meter::headroom();
    let values = array(vec![Val::Null; 4]);
    let retained = meter::headroom();
    assert!(retained < initial);
    let mut iter = values.into_iter();
    assert_eq!(
        meter::headroom(),
        retained,
        "iterator still owns the buffer"
    );
    let _ = iter.next();
    assert_eq!(
        meter::headroom(),
        retained,
        "partial consumption keeps capacity"
    );
    drop(iter);
    assert_eq!(meter::headroom(), initial);
}

#[tokio::test]
async fn synchronous_evaluation_observes_host_cancellation() {
    let mut bash = crate::Bash::builder()
        .limits(crate::ExecutionLimits::new().timeout(std::time::Duration::from_secs(5)))
        .build();
    let token = bash.cancellation_token();
    let cancel = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        token.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let start = std::time::Instant::now();
    let result = bash.exec("jq -n 'until(false; .)'").await;
    cancel.join().unwrap();
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert!(start.elapsed() < std::time::Duration::from_secs(3));
    // The host owns resetting the shared flag before starting another request.
    bash.cancellation_token()
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let recovered = bash.exec("jq -n '1 + 1'").await.unwrap();
    assert_eq!(recovered.stdout, "2\n");
}

#[test]
fn collection_charges_capacity_before_advancing_the_iterator() {
    let (_guard, _meter) = meter::install(64, None);
    let abort = std::panic::catch_unwind(|| {
        array((0..10).map(|index| {
            if index == 4 {
                assert_eq!(meter::headroom(), 0);
            }
            Val::Null
        }));
    });
    assert!(abort.is_err());
}

#[test]
fn lifecycle_traversals_poll_the_stop_check() {
    use std::hash::{Hash, Hasher};

    // Two separately built shared trees. Equality cannot use Rc identity to
    // skip their descendants. Small enough to finish even without polling.
    let tree = || {
        let mut value = Val::Null;
        for _ in 0..13 {
            value = [value.clone(), value].into_iter().collect();
        }
        value
    };
    let left = tree();
    let right = tree();
    for operation in ["equality", "ordering", "hash", "format"] {
        let (_guard, _meter) = meter::install(1 << 20, Some(Box::new(|| true)));
        let abort = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match operation {
            "equality" => {
                std::hint::black_box(left == right);
            }
            "ordering" => {
                std::hint::black_box(left.cmp(&right));
            }
            "hash" => {
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                left.hash(&mut hash);
                std::hint::black_box(hash.finish());
            }
            _ => {
                std::hint::black_box(left.to_string());
            }
        }));
        assert!(
            abort
                .is_err_and(|error| error.downcast_ref::<meter::Abort>()
                    == Some(&meter::Abort::Interrupted)),
            "{operation} did not poll cancellation"
        );
    }
}
