use aok::Void;
use sonic_rs::{JsonValueTrait, Value};
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  json::{
    JsonArrIndex, JsonMeta, JsonSet, JsonStorageFormat, PathSegment, delete_path_values,
    get_path_values, json_merge_patch, mutate_path_values, parse_json_path,
  },
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_json_set_get_del() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.json_set_one(
    "doc",
    "$",
    r#"{"a": 1, "b": "hello", "nested": {"k": "v"}}"#,
  )?;
  let val_b = db.json_get_one("doc", "$.b")?;
  assert_eq!(val_b, Some(r#"["hello"]"#.to_string()));

  let val_nested = db.json_get_one("doc", "$.nested.k")?;
  assert_eq!(val_nested, Some(r#"["v"]"#.to_string()));

  db.json_set_one("doc", "$.c", r#"[10, 20]"#)?;
  let val_c = db.json_get_one("doc", "$.c")?;
  assert_eq!(val_c, Some(r#"[[10,20]]"#.to_string()));

  assert_eq!(db.json_del("doc", Some("$.a"))?, 1);
  assert_eq!(db.json_get_one("doc", "$.a")?, Some("[]".to_string()));

  let full = db.json_get("doc", &[], [])?;
  assert!(full.is_some());

  assert_eq!(db.json_del("doc", None)?, 1);
  assert_eq!(db.json_get("doc", &[], [])?, None);

  // 尝试在不存在的 key 的非根路径设置（应报错）
  let err = db.json_set_one("new_key", "$.sub", r#"{"x": 1}"#);
  assert!(err.is_err());

  Ok(())
}

#[test]
fn test_json_advanced_ops() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. JSON.SET with NX / XX
  assert!(db.json_set(
    "user",
    "$",
    r#"{"name": "Alice", "age": 30, "active": true, "skills": ["rust", "c++"]}"#,
    [JsonSet::Nx]
  )?);
  assert!(!db.json_set("user", "$", r#"{"name": "Bob"}"#, [JsonSet::Nx])?);
  assert!(db.json_set("user", "$.name", r#""Alice Bob""#, [JsonSet::Xx])?);

  // 2. JSON.TYPE
  let t = db.json_type("user", Some("$.name"))?;
  assert_eq!(t, vec!["string"]);
  let t_age = db.json_type("user", Some("$.age"))?;
  assert_eq!(t_age, vec!["integer"]);
  let t_skills = db.json_type("user", Some("$.skills"))?;
  assert_eq!(t_skills, vec!["array"]);

  // 3. JSON.NUMINCRBY & NUMMULTBY
  let incr_res = db.json_numincrby("user", "$.age", "5")?;
  assert!(incr_res.is_some());
  let get_age = db.json_get_one("user", "$.age")?;
  assert_eq!(get_age, Some("[35]".to_string()));

  db.json_nummultby("user", "$.age", "2")?;
  let get_age_mult = db.json_get_one("user", "$.age")?;
  assert_eq!(get_age_mult, Some("[70]".to_string()));

  // 4. JSON.STRAPPEND & STRLEN
  db.json_strappend("user", Some("$.name"), " Jr.")?;
  let len = db.json_strlen("user", Some("$.name"))?;
  assert_eq!(len, vec![Some(13)]);

  // 5. JSON.ARRAPPEND, ARRLEN, ARRPOP, ARRTRIM
  db.json_arrappend("user", "$.skills", &[r#""go""#, r#""python""#])?;
  let arr_len = db.json_arrlen("user", Some("$.skills"))?;
  assert_eq!(arr_len, vec![Some(4)]);

  let popped = db.json_arrpop("user", Some("$.skills"), Some(-1))?;
  assert_eq!(popped, vec![Some(r#""python""#.to_string())]);

  db.json_arrtrim("user", "$.skills", 0, 1)?;
  let trim_len = db.json_arrlen("user", Some("$.skills"))?;
  assert_eq!(trim_len, vec![Some(2)]);

  // 6. JSON.TOGGLE
  let toggled = db.json_toggle("user", Some("$.active"))?;
  assert_eq!(toggled, vec![Some(false)]);

  // 7. JSON.OBJKEYS & OBJLEN
  let keys = db.json_objkeys("user", Some("$"))?;
  assert_eq!(keys.len(), 1);
  assert!(keys[0].as_ref().unwrap().contains(&"name".to_string()));

  let obj_len = db.json_objlen("user", Some("$"))?;
  assert_eq!(obj_len, vec![Some(4)]);

  // 8. JSON.MERGE (RFC 7396)
  db.json_merge("user", "$", r#"{"location": "Tokyo", "active": null}"#)?;
  let loc = db.json_get_one("user", "$.location")?;
  assert_eq!(loc, Some(r#"["Tokyo"]"#.to_string()));
  let active = db.json_get_one("user", "$.active")?;
  assert_eq!(active, Some("[]".to_string()));

  // 9. JSON.MGET
  db.json_set_one("user2", "$", r#"{"location": "Osaka"}"#)?;
  let mget = db.json_mget(&["user", "user2", "nonexistent"], "$.location")?;
  assert_eq!(mget.len(), 3);
  assert_eq!(mget[0], Some(r#"["Tokyo"]"#.to_string()));
  assert_eq!(mget[1], Some(r#"["Osaka"]"#.to_string()));
  assert_eq!(mget[2], None);

  // 10. JSON.INFO
  let info = db.json_info("user")?;
  assert!(info.is_some());
  assert_eq!(info.unwrap().0, JsonStorageFormat::Json);

  // 11. JSON.CLEAR
  let cleared = db.json_clear("user", Some("$.skills"))?;
  assert_eq!(cleared, 1);
  let skills_len = db.json_arrlen("user", Some("$.skills"))?;
  assert_eq!(skills_len, vec![Some(0)]);

  Ok(())
}

#[test]
fn test_json_arrinsert_and_arrindex_boundaries() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.json_set_one("arrdoc", "$", r#"{"nums": [10, 20, 30]}"#)?;

  // 1. ARRINSERT 正向与反向插入
  let ins1 = db.json_arrinsert("arrdoc", "$.nums", 1, &[r#"15"#])?;
  assert_eq!(ins1, vec![Some(4)]);
  let val1 = db.json_get_one("arrdoc", "$.nums")?;
  assert_eq!(val1, Some("[[10,15,20,30]]".to_string()));

  let ins2 = db.json_arrinsert("arrdoc", "$.nums", -1, &[r#"25"#])?;
  assert_eq!(ins2, vec![Some(5)]);
  let val2 = db.json_get_one("arrdoc", "$.nums")?;
  assert_eq!(val2, Some("[[10,15,20,25,30]]".to_string()));

  // 超出边界插入应返回 None
  let ins_out = db.json_arrinsert("arrdoc", "$.nums", 100, &[r#"999"#])?;
  assert_eq!(ins_out, vec![None]);

  // 2. ARRINDEX 检索与区间过滤
  let idx1 = db.json_arrindex("arrdoc", "$.nums", "20", [JsonArrIndex::Start(0)])?;
  assert_eq!(idx1, vec![Some(2)]);

  let idx2 = db.json_arrindex("arrdoc", "$.nums", "20", [JsonArrIndex::Start(3)])?;
  assert_eq!(idx2, vec![Some(-1)]); // 超出区间返回 -1

  let idx3 = db.json_arrindex("arrdoc", "$.nums", "999", [JsonArrIndex::Start(0)])?;
  assert_eq!(idx3, vec![Some(-1)]); // 不存在返回 -1

  Ok(())
}

#[test]
fn test_json_formatting_and_multi_paths() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.json_set_one(
    "complex",
    "$",
    r#"{"name": "Alice", "age": 28, "address": {"city": "Beijing", "zip": "100000"}}"#,
  )?;

  // 1. INDENT / NEWLINE / SPACE 格式化
  let pretty = db.json_get_formatted("complex", &["$"], Some("  "), Some("\n"), Some(" "))?;
  assert!(pretty.is_some());
  let formatted_str = pretty.unwrap();
  assert!(formatted_str.contains("  \"name\": \"Alice\""));
  assert!(formatted_str.contains("  \"address\": {"));

  // 2. 多路径查询返回映射对象
  let multi = db.json_get_formatted("complex", &["$.name", "$.address.city"], None, None, None)?;
  assert!(multi.is_some());
  let multi_str = multi.unwrap();
  assert!(multi_str.contains(r#""$.name":["Alice"]"#));
  assert!(multi_str.contains(r#""$.address.city":["Beijing"]"#));

  Ok(())
}

#[test]
fn test_json_mset_batch_atomic_dirty_keys() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 在单次 MSET 中连续修改同一个 key 的多个路径
  db.json_mset(&[
    ("k1", "$", r#"{"a": 1}"#),
    ("k2", "$", r#"{"x": 100}"#),
    ("k1", "$.b", r#"2"#),
    ("k1", "$.c", r#"3"#),
    ("k2", "$.y", r#"200"#),
  ])?;

  let k1_val = db.json_get("k1", &[], [])?;
  assert!(k1_val.is_some());
  let k1_str = k1_val.unwrap();
  assert!(k1_str.contains(r#""a":1"#));
  assert!(k1_str.contains(r#""b":2"#));
  assert!(k1_str.contains(r#""c":3"#));

  let k2_val = db.json_get("k2", &[], [])?;
  assert!(k2_val.is_some());
  let k2_str = k2_val.unwrap();
  assert!(k2_str.contains(r#""x":100"#));
  assert!(k2_str.contains(r#""y":200"#));

  Ok(())
}

#[test]
fn test_json_debug_and_resp() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  db.json_set_one(
    "doc",
    "$",
    r#"{"str": "hello", "num": 123, "flag": true, "arr": [1, 2]}"#,
  )?;

  // 1. JSON.DEBUG MEMORY
  let sizes = db.json_debug_memory("doc", Some("$"))?;
  assert_eq!(sizes.len(), 1);
  assert!(sizes[0] > 0);

  let sub_sizes = db.json_debug_memory("doc", Some("$.str"))?;
  assert_eq!(sub_sizes.len(), 1);
  assert_eq!(sub_sizes[0], r#""hello""#.len());

  Ok(())
}

#[test]
fn test_json_comprehensive_path_eval_and_filters() -> Void {
  // 1. JsonMeta 27 字节编码解码
  let segments = parse_json_path("$.store.book[0].title")?;
  assert_eq!(segments.len(), 5);

  let meta = JsonMeta::with_format(JsonStorageFormat::Json, 1000, 2, 50);
  assert_eq!(JsonMeta::ENCODED_SIZE, 27);
  let enc = meta.encode();
  assert_eq!(enc.len(), 27);
  let (dec, payload) = JsonMeta::decode(&enc).expect("decode failed");
  assert_eq!(dec.format, JsonStorageFormat::Json);
  assert_eq!(dec.base.expire_at, 1000);
  assert_eq!(dec.base.version, 2);
  assert!(payload.is_empty());

  // 2. RFC 7396 Merge Patch 规范测试用例
  let mut target: Value = sonic_rs::from_str(r#"{"a": "b", "c": {"d": "e", "f": "g"}}"#).unwrap();
  let patch: Value = sonic_rs::from_str(r#"{"a": "z", "c": {"f": null}}"#).unwrap();
  json_merge_patch(&mut target, &patch);

  let res_str = sonic_rs::to_string(&target).unwrap();
  assert!(res_str.contains(r#""a":"z""#));
  assert!(res_str.contains(r#""d":"e""#));
  assert!(!res_str.contains(r#""f""#));

  // 3. 复杂 JSONPath 解析与切片
  let book_doc: Value = sonic_rs::from_str(
    r#"{
        "store": {
            "book": [
                {"title": "Book 0", "price": 10.0, "category": "fiction"},
                {"title": "Book 1", "price": 25.0, "category": "tech"},
                {"title": "Book 2", "price": 50.0, "category": "tech"},
                {"title": "Book 3", "price": 80.0, "category": "science"}
            ]
        }
    }"#,
  )
  .unwrap();

  // 切片
  let slice_books = get_path_values(&book_doc, "$.store.book[1:3]")?;
  assert_eq!(slice_books.len(), 2);
  assert_eq!(slice_books[0]["title"].as_str(), Some("Book 1"));
  assert_eq!(slice_books[1]["title"].as_str(), Some("Book 2"));

  // 过滤器表达式
  let tech_books = get_path_values(&book_doc, r#"$.store.book[?(@.category == 'tech')]"#)?;
  assert_eq!(tech_books.len(), 2);

  let expensive_books = get_path_values(&book_doc, "$.store.book[?(@.price >= 50)]")?;
  assert_eq!(expensive_books.len(), 2);

  // 递归下降搜索
  let all_titles = get_path_values(&book_doc, "$..title")?;
  assert_eq!(all_titles.len(), 4);

  // 递归变异与删除
  let mut mut_doc = book_doc.clone();
  mutate_path_values(&mut mut_doc, "$.store.book[?(@.price < 30)].price", |p| {
    if let Some(f) = p.as_f64() {
      *p = sonic_rs::json!((f * 2.0) as i64);
    }
  })?;

  let updated_p0 = get_path_values(&mut_doc, "$.store.book[0].price")?;
  assert_eq!(updated_p0[0].as_i64(), Some(20));

  let del_cnt = delete_path_values(&mut mut_doc, "$.store.book[0]")?;
  assert_eq!(del_cnt, 1);
  let remaining_books = get_path_values(&mut_doc, "$.store.book[*]")?;
  assert_eq!(remaining_books.len(), 3);

  Ok(())
}

#[test]
fn test_json_kvrocks_cpp_compatibility() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. Set & Get 递归下降与通配符 (对标 Kvrocks RedisJsonTest.Set)
  db.json_set_one("k", "$", "[[1,2],[3,4],[5,6]]")?;
  db.json_set_one("k", "$[*][1]", r#""x""#)?;
  let val = db.json_get("k", &[], [])?;
  assert_eq!(val, Some(r#"[[1,"x"],[3,"x"],[5,"x"]]"#.to_string()));

  db.json_set_one("k", "$", r#"{"x":1,"y":{"a":"xxx","x":2},"z":3}"#)?;
  db.json_set_one("k", "$..x", "true")?;
  let val2 = db.json_get("k", &[], [])?;
  let parsed_v2: Value = sonic_rs::from_str(&val2.unwrap()).unwrap();
  let expected_v2: Value =
    sonic_rs::from_str(r#"{"x":true,"y":{"a":"xxx","x":true},"z":3}"#).unwrap();
  assert_eq!(parsed_v2, expected_v2);

  db.json_set_one("k", "$", "[[1,2],[[5,6],4]]")?;
  db.json_set_one("k", "$..[0]", "{}")?;
  let val3 = db.json_get("k", &[], [])?;
  assert_eq!(val3, Some(r#"[{},[{},4]]"#.to_string()));

  // 2. 递归 Get (对标 Kvrocks RedisJsonTest.Get)
  let doc: Value = sonic_rs::from_str(r#"[[[1,2],[3]],[4,5]]"#).unwrap();
  let res = get_path_values(&doc, "$..[0]")?;
  let res_str = sonic_rs::to_string(&res).unwrap();
  assert_eq!(res_str, r#"[[[1,2],[3]],[1,2],1,3,4]"#);

  // 3. ArrPop (对标 Kvrocks RedisJsonTest.ArrPop)
  db.json_set_one("pop_doc", "$", r#"[3,"str",2.1,{},[5,6]]"#)?;
  let p1 = db.json_arrpop("pop_doc", Some("$"), Some(-1))?;
  assert_eq!(p1, vec![Some("[5,6]".to_string())]);

  let p2 = db.json_arrpop("pop_doc", Some("$"), Some(-2))?;
  assert_eq!(p2, vec![Some("2.1".to_string())]);

  let p3 = db.json_arrpop("pop_doc", Some("$"), Some(3))?;
  assert_eq!(p3, vec![Some("{}".to_string())]);

  let p4 = db.json_arrpop("pop_doc", Some("$"), Some(1))?;
  assert_eq!(p4, vec![Some(r#""str""#.to_string())]);

  let p5 = db.json_arrpop("pop_doc", Some("$"), Some(0))?;
  assert_eq!(p5, vec![Some("3".to_string())]);

  let p6 = db.json_arrpop("pop_doc", Some("$"), Some(-1))?;
  assert_eq!(p6, vec![None]);

  // 4. Toggle (对标 Kvrocks RedisJsonTest.Toggle)
  db.json_set_one(
    "toggle_doc",
    "$",
    r#"{"bool":false,"bools":{"bool":true},"incorrectbool":{"bool":88}}"#,
  )?;
  let tog = db.json_toggle("toggle_doc", Some("$..bool"))?;
  assert_eq!(tog.len(), 3);
  assert!(tog.contains(&Some(true)));
  assert!(tog.contains(&Some(false)));
  assert!(tog.contains(&None));

  // 5. Clear (对标 Kvrocks RedisJsonTest.Clear)
  db.json_set_one(
    "clear_doc",
    "$",
    r#"{"obj":{"a":1, "b":2}, "arr":[1,2,3], "str": "foo", "bool": true, "int": 42, "float": 3.14}"#,
  )?;
  let clr = db.json_clear("clear_doc", Some("$.*"))?;
  assert_eq!(clr, 4);
  let clr_res = db.json_get("clear_doc", &[], [])?;
  let parsed_clr: Value = sonic_rs::from_str(&clr_res.unwrap()).unwrap();
  let expected_clr: Value =
    sonic_rs::from_str(r#"{"arr":[],"bool":true,"float":0,"int":0,"obj":{},"str":"foo"}"#).unwrap();
  assert_eq!(parsed_clr, expected_clr);

  // 6. NumIncrBy / NumMultBy 边界与无穷大溢出校验
  db.json_set_one("num_doc", "$", r#"{"foo": 0, "bar": "baz"}"#)?;
  let r1 = db.json_numincrby("num_doc", "$.foo", "1")?;
  assert_eq!(r1, Some("[1]".to_string()));

  let r2 = db.json_numincrby("num_doc", "$.bar", "1")?;
  assert_eq!(r2, Some("[null]".to_string()));

  let r3 = db.json_numincrby("num_doc", "$.fuzz", "1")?;
  assert_eq!(r3, Some("[]".to_string()));

  // 溢出检查
  db.json_set_one("big_num", "$", "1.6350000000001313e+308")?;
  let inf_err = db.json_nummultby("big_num", "$", "2");
  assert!(inf_err.is_err());

  Ok(())
}

#[test]
fn test_json_edge_cases_and_deep_recursion() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 递归下降删除带尾部路径 ($..store.book)
  let mut data: Value = sonic_rs::from_str(
    r#"{"root": {"store": {"book": {"title": "Rust"}}, "other": {"store": {"book": {"title": "Go"}}}}}"#,
  )?;
  let del_cnt = delete_path_values(&mut data, "$..store.book")?;
  assert_eq!(del_cnt, 2);
  let check = get_path_values(&data, "$..book")?;
  assert!(check.is_empty());

  // 2. 括号路径规范创建与深层设值
  db.json_set_one("bracket_doc", "$", r#"{"a": {}}"#)?;
  db.json_set_one("bracket_doc", "$['a']['b']", r#"42"#)?;
  let b_val = db.json_get_one("bracket_doc", "$.a.b")?;
  assert_eq!(b_val, Some("[42]".to_string()));

  // 3. 过滤器跨整型/浮点数相等性比较
  let num_data: Value = sonic_rs::from_str(r#"[{"x": 10}, {"x": 20.0}, {"x": 30}]"#)?;
  let matched_float = get_path_values(&num_data, "$[?(@.x == 20.0)]")?;
  assert_eq!(matched_float.len(), 1);
  assert_eq!(matched_float[0]["x"].as_f64(), Some(20.0));

  let matched_int = get_path_values(&num_data, "$[?(@.x == 10.0)]")?;
  assert_eq!(matched_int.len(), 1);
  assert_eq!(matched_int[0]["x"].as_i64(), Some(10));

  // 4. 无 $ 前缀的通配符与递归分段解析
  let wildcard_segs = parse_json_path("*")?;
  assert_eq!(wildcard_segs, vec![PathSegment::Wildcard]);

  let wild_obj = sonic_rs::json!({"a": 1, "b": 2});
  let res_wild = get_path_values(&wild_obj, "*")?;
  assert_eq!(res_wild.len(), 2);

  // 5. 零内存分配元数据头验证
  let meta = JsonMeta::with_format(JsonStorageFormat::Json, 5000, 100, 64);
  let enc = meta.encode();
  assert_eq!(enc.len(), 27);
  let (dec, _) = JsonMeta::decode(&enc).unwrap();
  assert_eq!(dec.base.expire_at, 5000);
  assert_eq!(dec.base.size, 64);

  // 6. 混合 Index 与 Field 的非存在路径定位创建
  db.json_set_one("mixed_nest", "$", r#"{"arr": [{}]}"#)?;
  db.json_set_one("mixed_nest", "$.arr[0].key", r#""val""#)?;
  let mixed_val = db.json_get_one("mixed_nest", "$.arr[0].key")?;
  assert_eq!(mixed_val, Some(r#"["val"]"#.to_string()));

  db.json_set_one("mixed_nest", "$.arr[1]", r#"{"next": 99}"#)?;
  let arr1_val = db.json_get_one("mixed_nest", "$.arr[1].next")?;
  assert_eq!(arr1_val, Some("[99]".to_string()));

  // 8. STRAPPEND 类型严格检查（非字符串 JSON 输入报错）
  let append_err = db.json_strappend("mixed_nest", Some("$.arr[0].key"), "123");
  assert!(append_err.is_err());

  Ok(())
}

#[test]
fn test_json_slice_negative_step_and_formatting() -> Void {
  let dir = tempdir()?;
  let db = WeDb::new(Fjall::open(dir.path())?).ns(0)?.db(0)?;

  // 1. 数组切片负步长求值与变异
  let mut data: Value = sonic_rs::from_str(r#"{"arr": [0, 1, 2, 3, 4, 5]}"#)?;
  let slice_rev = get_path_values(&data, "$.arr[::-1]")?;
  assert_eq!(slice_rev.len(), 6);
  assert_eq!(slice_rev[0].as_i64(), Some(5));
  assert_eq!(slice_rev[5].as_i64(), Some(0));

  mutate_path_values(&mut data, "$.arr[4:1:-2]", |v| {
    if let Some(i) = v.as_i64() {
      *v = sonic_rs::json!(i * 10);
    }
  })?;
  assert_eq!(data["arr"][4].as_i64(), Some(40));
  assert_eq!(data["arr"][2].as_i64(), Some(20));
  assert_eq!(data["arr"][0].as_i64(), Some(0));

  // 2. 数组切片负步长删除
  let del_cnt = delete_path_values(&mut data, "$.arr[5:1:-2]")?;
  assert_eq!(del_cnt, 2); // 删除了索引 5 和 3

  // 3. 严格 SPACE 格式化测试（仅冒号后加空格，逗号后不加空格）
  db.json_set_one("fmt_doc", "$", r#"{"x":1,"y":2}"#)?;
  let space_fmt = db.json_get_formatted("fmt_doc", &[], None, None, Some(" "))?;
  assert_eq!(space_fmt, Some(r#"{"x": 1,"y": 2}"#.to_string()));

  Ok(())
}
