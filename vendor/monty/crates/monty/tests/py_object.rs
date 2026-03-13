use monty::MontyObject;

/// Tests for `MontyObject::is_truthy()` - Python's truth value testing rules.

#[test]
fn is_truthy_none_is_falsy() {
    assert!(!MontyObject::None.is_truthy());
}

#[test]
fn is_truthy_ellipsis_is_truthy() {
    assert!(MontyObject::Ellipsis.is_truthy());
}

#[test]
fn is_truthy_false_is_falsy() {
    assert!(!MontyObject::Bool(false).is_truthy());
}

#[test]
fn is_truthy_true_is_truthy() {
    assert!(MontyObject::Bool(true).is_truthy());
}

#[test]
fn is_truthy_zero_int_is_falsy() {
    assert!(!MontyObject::Int(0).is_truthy());
}

#[test]
fn is_truthy_nonzero_int_is_truthy() {
    assert!(MontyObject::Int(1).is_truthy());
    assert!(MontyObject::Int(-1).is_truthy());
    assert!(MontyObject::Int(42).is_truthy());
}

#[test]
fn is_truthy_zero_float_is_falsy() {
    assert!(!MontyObject::Float(0.0).is_truthy());
}

#[test]
fn is_truthy_nonzero_float_is_truthy() {
    assert!(MontyObject::Float(1.0).is_truthy());
    assert!(MontyObject::Float(-0.5).is_truthy());
    assert!(MontyObject::Float(f64::INFINITY).is_truthy());
}

#[test]
fn is_truthy_empty_string_is_falsy() {
    assert!(!MontyObject::String(String::new()).is_truthy());
}

#[test]
fn is_truthy_nonempty_string_is_truthy() {
    assert!(MontyObject::String("hello".to_string()).is_truthy());
    assert!(MontyObject::String(" ".to_string()).is_truthy());
}

#[test]
fn is_truthy_empty_bytes_is_falsy() {
    assert!(!MontyObject::Bytes(vec![]).is_truthy());
}

#[test]
fn is_truthy_nonempty_bytes_is_truthy() {
    assert!(MontyObject::Bytes(vec![0]).is_truthy());
    assert!(MontyObject::Bytes(vec![1, 2, 3]).is_truthy());
}

#[test]
fn is_truthy_empty_list_is_falsy() {
    assert!(!MontyObject::List(vec![]).is_truthy());
}

#[test]
fn is_truthy_nonempty_list_is_truthy() {
    assert!(MontyObject::List(vec![MontyObject::Int(1)]).is_truthy());
}

#[test]
fn is_truthy_empty_tuple_is_falsy() {
    assert!(!MontyObject::Tuple(vec![]).is_truthy());
}

#[test]
fn is_truthy_nonempty_tuple_is_truthy() {
    assert!(MontyObject::Tuple(vec![MontyObject::Int(1)]).is_truthy());
}

#[test]
fn is_truthy_empty_dict_is_falsy() {
    assert!(!MontyObject::dict(vec![]).is_truthy());
}

#[test]
fn is_truthy_nonempty_dict_is_truthy() {
    let dict = vec![(MontyObject::String("key".to_string()), MontyObject::Int(1))];
    assert!(MontyObject::dict(dict).is_truthy());
}

/// Tests for `MontyObject::type_name()` - Python type names.

#[test]
fn type_name() {
    assert_eq!(MontyObject::None.type_name(), "NoneType");
    assert_eq!(MontyObject::Ellipsis.type_name(), "ellipsis");
    assert_eq!(MontyObject::Bool(true).type_name(), "bool");
    assert_eq!(MontyObject::Bool(false).type_name(), "bool");
    assert_eq!(MontyObject::Int(0).type_name(), "int");
    assert_eq!(MontyObject::Int(42).type_name(), "int");
    assert_eq!(MontyObject::Float(0.0).type_name(), "float");
    assert_eq!(MontyObject::Float(2.5).type_name(), "float");
    assert_eq!(MontyObject::String(String::new()).type_name(), "str");
    assert_eq!(MontyObject::String("hello".to_string()).type_name(), "str");
    assert_eq!(MontyObject::Bytes(vec![]).type_name(), "bytes");
    assert_eq!(MontyObject::Bytes(vec![1, 2, 3]).type_name(), "bytes");
    assert_eq!(MontyObject::List(vec![]).type_name(), "list");
    assert_eq!(MontyObject::Tuple(vec![]).type_name(), "tuple");
    assert_eq!(MontyObject::dict(vec![]).type_name(), "dict");
}
