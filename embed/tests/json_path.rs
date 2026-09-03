use sonic_rs::{JsonValueTrait, json};
use wedb_embed::{delete_path_values, extract_simple_field, get_path_values, mutate_path_values};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_extract_simple_field() {
  assert_eq!(extract_simple_field("$.field"), Some("field"));
  assert_eq!(extract_simple_field(".field"), Some("field"));
  assert_eq!(extract_simple_field("field"), Some("field"));
  assert_eq!(extract_simple_field("  $.name  "), Some("name"));

  // 不应作为单层简单字段快速路径的情况
  assert_eq!(extract_simple_field("$"), None);
  assert_eq!(extract_simple_field("."), None);
  assert_eq!(extract_simple_field(""), None);
  assert_eq!(extract_simple_field("$.a.b"), None);
  assert_eq!(extract_simple_field("$.arr[0]"), None);
  assert_eq!(extract_simple_field("$[0]"), None);
  assert_eq!(extract_simple_field("$.*"), None);
  assert_eq!(extract_simple_field("*"), None);
  assert_eq!(extract_simple_field("$[?(@.x == 1)]"), None);
}

#[test]
fn test_simple_field_fast_path_eval_and_mutate_and_delete() {
  let mut doc = json!({
    "name": "Alice",
    "age": 30,
    "nested": {"key": "value"}
  });

  // get 快速路径
  let name_vals = get_path_values(&doc, "$.name").unwrap();
  assert_eq!(name_vals.len(), 1);
  assert_eq!(name_vals[0].as_str(), Some("Alice"));

  let non_exist = get_path_values(&doc, "$.missing").unwrap();
  assert!(non_exist.is_empty());

  // mutate 快速路径
  let count = mutate_path_values(&mut doc, "$.age", |v| {
    *v = json!(31);
  })
  .unwrap();
  assert_eq!(count, 1);
  assert_eq!(doc["age"].as_i64(), Some(31));

  // delete 快速路径
  let del_cnt = delete_path_values(&mut doc, "$.name").unwrap();
  assert_eq!(del_cnt, 1);
  assert!(doc.get("name").is_none());

  let del_non_exist = delete_path_values(&mut doc, "$.non_exist").unwrap();
  assert_eq!(del_non_exist, 0);
}
