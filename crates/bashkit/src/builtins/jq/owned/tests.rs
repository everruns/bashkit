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
    assert!(matches!(result, Err(crate::Error::Cancelled)));
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
fn nested_meter_restores_poll_progress() {
    let outer_calls = Rc::new(std::cell::Cell::new(0));
    let calls = outer_calls.clone();
    let (_guard, _meter) = meter::install(
        4096,
        Some(Box::new(move || {
            calls.set(calls.get() + 1);
            false
        })),
    );
    for _ in 0..meter::TICKS_PER_POLL - 1 {
        meter::tick();
    }
    {
        let (_inner_guard, _inner_meter) = meter::install(4096, Some(Box::new(|| false)));
        for _ in 0..meter::TICKS_PER_POLL + 1 {
            meter::tick();
        }
    }
    assert_eq!(outer_calls.get(), 0);
    meter::tick();
    assert_eq!(outer_calls.get(), 1);
}

#[test]
fn array_growth_uses_the_remaining_budget_without_over_reserving() {
    let (_guard, meter) = meter::install(6 * size_of::<Val>(), None);
    let mut values = Container::<Vec<Val>>::default();
    for index in 0..6 {
        values.set(index, Val::from(index)).unwrap();
    }
    assert_eq!(values.len(), 6);
    assert_eq!(meter::headroom(), 0);
    assert!(!meter.tripped());
}

#[test]
fn a_single_element_can_use_the_exact_remaining_budget() {
    let (_guard, meter) = meter::install(size_of::<Val>(), None);
    let mut values = Container::<Vec<Val>>::default();
    values.push(Val::Null).unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(meter::headroom(), 0);
    assert!(!meter.tripped());
}

#[test]
fn replacing_a_full_object_does_not_reserve_more_storage() {
    let (_guard, _meter) = meter::install(4096, None);
    let mut object = Container::<Map>::default();
    object.insert(Val::utf8_str("0"), Val::Null).unwrap();
    for index in 1..object.capacity() {
        object
            .insert(Val::from(index.to_string()), Val::Null)
            .unwrap();
    }
    assert_eq!(object.len(), object.capacity());
    let available = meter::headroom();
    let key = object.keys().next().unwrap().clone();
    object.insert(key, Val::Bool(true)).unwrap();
    assert_eq!(meter::headroom(), available);
}

#[test]
fn absent_object_updates_do_not_allocate_before_the_callback_result() {
    let key = Val::utf8_str("key");
    let error = Error::str("callback failed");
    let (_guard, meter) = meter::install(0, None);
    let mut object = Container::<Map>::default();
    object
        .update(key.clone(), |_| Ok::<_, Error>(None))
        .unwrap();
    assert!(object.update(key, |_| Err(error)).is_err());
    assert_eq!(object.capacity(), 0);
    assert!(object.is_empty());
    assert!(!meter.tripped());
}

fn decoded(value: Val) -> serde_json::Value {
    serde_json::from_str(&value.to_string()).unwrap()
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(64))]

    #[test]
    fn arrays_match_native_model(actions in proptest::collection::vec((0u8..5, 0usize..64, -1000isize..1000), 0..128)) {
        let (_guard, _meter) = meter::install(4 * 1024 * 1024, None);
        let initial = meter::headroom();
        let mut actual = Rc::new(Container::<Vec<Val>>::default());
        let mut model: Vec<Option<isize>> = Vec::new();
        for (operation, index, value) in actions {
            match operation {
                0 | 4 => {
                    let snapshot = (operation == 4).then(|| actual.clone());
                    let before = serde_json::to_value(&model).unwrap();
                    Rc::make_mut(&mut actual).set(index, Val::from(value)).unwrap();
                    if let Some(snapshot) = snapshot {
                        proptest::prop_assert_eq!(decoded(Val::Arr(snapshot)), before);
                    }
                    model.resize(model.len().max(index + 1), None);
                    model[index] = Some(value);
                }
                1 if !model.is_empty() => {
                    let index = index % model.len();
                    Rc::make_mut(&mut actual).remove(index);
                    model.remove(index);
                }
                2 if !model.is_empty() => {
                    let index = index % model.len();
                    let old = Rc::make_mut(&mut actual).take(index);
                    proptest::prop_assert_eq!(decoded(old), serde_json::to_value(model[index].take()).unwrap());
                }
                3 => {
                    let skip = index % (model.len() + 1);
                    let take = (index % 4).min(model.len() - skip);
                    let replacement = array([Val::from(value)]);
                    Rc::make_mut(&mut actual).splice(skip, take, &replacement).unwrap();
                    model.splice(skip..skip + take, [Some(value)]);
                }
                _ => {}
            }
            proptest::prop_assert_eq!(decoded(Val::Arr(actual.clone())), serde_json::to_value(&model).unwrap());
        }
        drop(actual);
        proptest::prop_assert_eq!(meter::headroom(), initial);
    }

    #[test]
    fn objects_match_native_model(actions in proptest::collection::vec((0u8..5, 0usize..32, -1000isize..1000), 0..128)) {
        let (_guard, _meter) = meter::install(4 * 1024 * 1024, None);
        let initial = meter::headroom();
        let mut actual = Rc::new(Container::<Map>::default());
        let mut model = std::collections::BTreeMap::<String, Option<isize>>::new();
        for (operation, index, value) in actions {
            let key = index.to_string();
            match operation {
                0 | 3 => {
                    let snapshot = (operation == 3).then(|| actual.clone());
                    let before = serde_json::to_value(&model).unwrap();
                    Rc::make_mut(&mut actual).insert(Val::from(key.clone()), Val::from(value)).unwrap();
                    if let Some(snapshot) = snapshot {
                        proptest::prop_assert_eq!(decoded(Val::Obj(snapshot)), before);
                    }
                    model.insert(key, Some(value));
                }
                1 => {
                    Rc::make_mut(&mut actual).update(Val::from(key.clone()), |_| Ok::<_, Error>(None)).unwrap();
                    model.remove(&key);
                }
                2 => {
                    let mut old = None;
                    Rc::make_mut(&mut actual).update(Val::from(key.clone()), |value| {
                        old = Some(value);
                        Ok::<_, Error>(Some(Val::Null))
                    }).unwrap();
                    let expected = model.insert(key, None).flatten();
                    proptest::prop_assert_eq!(decoded(old.unwrap()), serde_json::to_value(expected).unwrap());
                }
                _ => {
                    let Val::Obj(right) = object([(Val::from(key.clone()), Val::from(value))]).unwrap() else { unreachable!() };
                    merge(&mut actual, right).unwrap();
                    model.insert(key, Some(value));
                }
            }
            proptest::prop_assert_eq!(decoded(Val::Obj(actual.clone())), serde_json::to_value(&model).unwrap());
        }
        drop(actual);
        proptest::prop_assert_eq!(meter::headroom(), initial);
    }
}
