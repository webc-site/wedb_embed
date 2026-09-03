use hipstr::HipStr;
use rapidhash::{RapidHashMap, RapidHashSet};
use wedb_embed::search::{
  DEFAULT_STOP_WORDS, DistanceMetric, FtAggregate, FtCreate, FtGroupBy, FtReducer, FtSearch,
  HnswGraph, IndexField, IndexFieldType, IndexOnDataType, InvertedIndex, SearchIndexManager,
  SearchIndexSchema, SearchKey, SearchQueryNode, SuggestionDict, VectorType,
  compute_vector_distance, decode_index_meta, decode_index_prefixes, decode_numeric_field_meta,
  decode_sortable_f64, decode_sortable_i64, decode_tag_field_meta, encode_index_meta,
  encode_index_prefixes, encode_numeric_field_meta, encode_sortable_f64, encode_sortable_i64,
  encode_tag_field_meta, explain_search_query, explain_search_query_cli, extract_doc_terms,
  levenshtein_distance, parse_search_query, parse_search_query_with_params,
  parse_vector_from_slice, tokenize_tags, tokenize_text, tokenize_text_with_stopwords,
  unescape_tag_string,
};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_search_query_parsing_and_explain() {
  let q = "@title:hello @tag:{rust | database} @age:[18 (30] -world";
  let ast = parse_search_query(q);
  let plan = explain_search_query(&ast);
  assert!(plan.contains("INTERSECT"));
  assert!(plan.contains("UNION <title:hello>"));
  assert!(plan.contains("TAG <@tag:{rust | database}>"));
  assert!(plan.contains("NUMERIC <@age:[18 30)>"));
  assert!(plan.contains("NOT {"));

  let cli_plan = explain_search_query_cli(&ast);
  assert_eq!(plan, cli_plan);
}

#[test]
fn test_query_knn_and_vector_range_parsing() {
  let mut params = RapidHashMap::default();
  let vec = [1.0f64, 2.0, 3.0, 4.0];
  let bytes: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
  params.insert("v".to_string(), unsafe {
    String::from_utf8_unchecked(bytes)
  });
  params.insert("k_num".to_string(), "5".to_string());
  params.insert("radius_val".to_string(), "0.75".to_string());

  let q = "*=>[KNN $k_num @embedding $v]";
  let ast = parse_search_query_with_params(q, &params);
  if let SearchQueryNode::VectorKnn {
    field,
    k,
    vector_param,
    vector,
  } = ast
  {
    assert_eq!(field, "embedding");
    assert_eq!(k, 5);
    assert_eq!(vector_param, "v");
    assert_eq!(vector, Some(vec![1.0, 2.0, 3.0, 4.0]));
  } else {
    panic!("expected VectorKnn node");
  }

  // 验证带空格的 KNN 箭头表达式及前置过滤条件
  let q_spaces = "(@category:{tech}) => [KNN 10 @embedding $v]";
  let ast_spaces = parse_search_query_with_params(q_spaces, &params);
  if let SearchQueryNode::And(children) = ast_spaces {
    assert_eq!(children.len(), 2);
    assert!(
      matches!(&children[0], SearchQueryNode::Tag { field, tags } if field == "category" && tags == &["tech"])
    );
    assert!(
      matches!(&children[1], SearchQueryNode::VectorKnn { field, k, .. } if field == "embedding" && *k == 10)
    );
  } else {
    panic!("expected And node with Tag filter and VectorKnn");
  }

  let q_range = "@embedding:[VECTOR_RANGE $radius_val $v]";
  let ast_range = parse_search_query_with_params(q_range, &params);
  if let SearchQueryNode::VectorRange {
    field,
    radius,
    vector_param,
    vector,
  } = ast_range
  {
    assert_eq!(field, "embedding");
    assert!((radius - 0.75).abs() < 1e-6);
    assert_eq!(vector_param, "v");
    assert_eq!(vector, Some(vec![1.0, 2.0, 3.0, 4.0]));
  } else {
    panic!("expected VectorRange node");
  }
}

#[test]
fn test_sortable_f64_and_i64_encoding() {
  let numbers = vec![-100.5, -0.01, 0.0, 0.001, 42.0, 9999.99];
  let encoded: Vec<String> = numbers.iter().map(|&n| encode_sortable_f64(n)).collect();
  let mut sorted_encoded = encoded.clone();
  sorted_encoded.sort();
  assert_eq!(encoded, sorted_encoded);

  for n in numbers {
    let enc = encode_sortable_f64(n);
    let dec = decode_sortable_f64(&enc).unwrap();
    assert!((n - dec).abs() < 1e-9);
  }

  let ints = vec![-999999i64, -42, 0, 1, 100, 123456789];
  let enc_ints: Vec<String> = ints.iter().map(|&i| encode_sortable_i64(i)).collect();
  let mut sorted_enc_ints = enc_ints.clone();
  sorted_enc_ints.sort();
  assert_eq!(enc_ints, sorted_enc_ints);

  for i in ints {
    let enc = encode_sortable_i64(i);
    let dec = decode_sortable_i64(&enc).unwrap();
    assert_eq!(i, dec);
  }
}

#[test]
fn test_tokenize_text_and_tags_and_stopwords() {
  let words = tokenize_text("Hello, RediSearch 2.0_beta in Rust!");
  assert_eq!(
    words,
    vec!["hello", "redisearch", "2", "0_beta", "in", "rust"]
  );

  let sw_set: RapidHashSet<String> = DEFAULT_STOP_WORDS.iter().map(|&s| s.to_string()).collect();
  let filtered_words =
    tokenize_text_with_stopwords("this is a test with stop words", Some(&sw_set));
  assert_eq!(filtered_words, vec!["test", "stop", "words"]);

  let tags = tokenize_tags("db, kv , redis, raft", ',', false);
  assert_eq!(tags, vec!["db", "kv", "redis", "raft"]);

  let case_tags = tokenize_tags("Redis, Raft, SQLite", ',', true);
  assert_eq!(case_tags, vec!["Redis", "Raft", "SQLite"]);

  assert_eq!(
    unescape_tag_string(r"email\@example\.com"),
    "email@example.com"
  );
  assert_eq!(unescape_tag_string(r"Hello\ World"), "Hello World");
}

#[test]
fn test_vector_distance_calculations() {
  let v1 = vec![1.0, 0.0, 0.0];
  let v2 = vec![0.0, 1.0, 0.0];
  let v3 = vec![1.0, 0.0, 0.0];

  // L2 (欧几里得距离)
  let dist_l2 = compute_vector_distance(&v1, &v2, DistanceMetric::L2).unwrap();
  assert!((dist_l2 - (2.0f64).sqrt()).abs() < 1e-6);

  // IP (内积度量，取负值)
  let dist_ip = compute_vector_distance(&v1, &v3, DistanceMetric::IP).unwrap();
  assert!((dist_ip - (-1.0)).abs() < 1e-6);

  // Cosine (余弦距离 1 - cos_sim)
  let dist_cos_same = compute_vector_distance(&v1, &v3, DistanceMetric::Cosine).unwrap();
  assert!(dist_cos_same.abs() < 1e-6);

  let dist_cos_ortho = compute_vector_distance(&v1, &v2, DistanceMetric::Cosine).unwrap();
  assert!((dist_cos_ortho - 1.0).abs() < 1e-6);

  // 二进制向量解析
  let bytes: Vec<u8> = [1.5f64, -2.5, 3.25]
    .iter()
    .flat_map(|f| f.to_le_bytes())
    .collect();
  let parsed = parse_vector_from_slice(&bytes, VectorType::Float64).unwrap();
  assert_eq!(parsed, vec![1.5, -2.5, 3.25]);

  // Float32 解析
  let bytes_f32: Vec<u8> = [1.5f32, -2.5, 3.25]
    .iter()
    .flat_map(|f| f.to_le_bytes())
    .collect();
  let parsed_f32 = parse_vector_from_slice(&bytes_f32, VectorType::Float32).unwrap();
  assert_eq!(parsed_f32, vec![1.5, -2.5, 3.25]);
}

#[test]
fn test_levenshtein_distance() {
  assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
  assert_eq!(levenshtein_distance("rust", "rust"), 0);
  assert_eq!(levenshtein_distance("redis", "reddis"), 1);
}

#[test]
fn test_inverted_index_indexing_and_search() {
  let schema = SearchIndexSchema::with_full_spec(
    "idx_books".to_string(),
    IndexOnDataType::Json,
    vec!["book:".to_string()],
    vec![
      IndexField::new("title", IndexFieldType::Text),
      IndexField::with_tag("category", Some(','), false),
      IndexField::with_numeric("price", true),
    ],
  );

  let mut idx = InvertedIndex::new();

  let doc1 = sonic_rs::json!({
      "title": "Rust Programming in Depth",
      "category": "tech, programming",
      "price": 49.9
  });
  let raw1 = sonic_rs::to_vec(&doc1).unwrap();
  idx
    .index_doc(&schema, "book:1", &raw1, Some(1.0), None)
    .unwrap();

  let doc2 = sonic_rs::json!({
      "title": "Distributed Databases and Consensus Algorithms",
      "category": "tech, database",
      "price": 89.0
  });
  let raw2 = sonic_rs::to_vec(&doc2).unwrap();
  idx
    .index_doc(&schema, "book:2", &raw2, Some(2.0), None)
    .unwrap();

  let doc3 = sonic_rs::json!({
      "title": "Cooking Masterclass Recipes",
      "category": "lifestyle, food",
      "price": 25.0
  });
  let raw3 = sonic_rs::to_vec(&doc3).unwrap();
  idx
    .index_doc(&schema, "book:3", &raw3, Some(0.5), None)
    .unwrap();

  // 1. 词条查询 "rust"
  let res = idx.search(&schema, "rust", &FtSearch::default()).unwrap();
  assert_eq!(res.total_results, 1);
  assert_eq!(res.docs[0].id, "book:1".to_string());

  // 2. 复合查询：@category:{tech} @price:[40 100]
  let res2 = idx
    .search(
      &schema,
      "@category:{tech} @price:[40 100]",
      &FtSearch::default(),
    )
    .unwrap();
  assert_eq!(res2.total_results, 2);

  // 3. 排除查询：@category:{tech} -consensus
  let res3 = idx
    .search(&schema, "@category:{tech} -consensus", &FtSearch::default())
    .unwrap();
  assert_eq!(res3.total_results, 1);
  assert_eq!(res3.docs[0].id, "book:1".to_string());

  // 4. 排序与分页：SORTBY price ASC LIMIT 0 2
  let search_opts = FtSearch {
    sortby: Some(("price".to_string(), true)),
    limit: Some((0, 2)),
    ..Default::default()
  };
  let res4 = idx.search(&schema, "*", &search_opts).unwrap();
  assert_eq!(res4.total_results, 3);
  assert_eq!(res4.docs.len(), 2);
  assert_eq!(res4.docs[0].id, "book:3".to_string()); // price 25.0
  assert_eq!(res4.docs[1].id, "book:1".to_string()); // price 49.9

  // 5. 投影字段 RETURN 1 title
  let search_return = FtSearch {
    returns: vec![("title".to_string(), None)],
    ..Default::default()
  };
  let res5 = idx.search(&schema, "databases", &search_return).unwrap();
  assert_eq!(res5.total_results, 1);
  assert_eq!(res5.docs[0].fields.len(), 1);
  assert_eq!(res5.docs[0].fields[0].0, "title".to_string());

  // 6. TAGVALS
  let tag_vals = idx.tag_vals("category");
  assert!(tag_vals.contains(&"tech".to_string()));
  assert!(tag_vals.contains(&"programming".to_string()));
  assert!(tag_vals.contains(&"database".to_string()));
  assert!(tag_vals.contains(&"lifestyle".to_string()));

  // 7. FT.INFO
  let info = idx.info(&schema);
  assert_eq!(info.index_name, "idx_books");
  assert_eq!(info.num_docs, 3);

  // 8. DELETE DOC
  let deleted = idx.delete_doc(&schema, "book:3");
  assert!(deleted);
  assert_eq!(idx.docs.len(), 2);
}

#[test]
fn test_search_index_manager_and_aliases_and_config() {
  let mut mgr = SearchIndexManager::new();

  let schema = SearchIndexSchema::with_full_spec(
    "users_idx".to_string(),
    IndexOnDataType::Hash,
    vec!["user:".to_string()],
    vec![
      IndexField::new("name", IndexFieldType::Text),
      IndexField::with_numeric("age", true),
    ],
  );

  // 1. FT.CREATE
  mgr.create_index(schema).unwrap();
  assert_eq!(mgr.list_indexes(), vec!["users_idx".to_string()]);

  // 2. 重复创建报错
  let dup_schema = SearchIndexSchema::new(
    "users_idx".to_string(),
    vec!["user:".to_string()],
    vec!["name".to_string()],
  );
  assert!(mgr.create_index(dup_schema).is_err());

  // 3. FT.ALIASADD / ALIASUPDATE / ALIASDEL
  mgr.add_alias("users_alias", "users_idx").unwrap();
  assert_eq!(mgr.resolve_index_name("users_alias"), "users_idx");

  mgr.update_alias("users_alias", "users_idx").unwrap();
  assert_eq!(mgr.resolve_index_name("users_alias"), "users_idx");

  mgr.del_alias("users_alias").unwrap();
  assert_eq!(mgr.resolve_index_name("users_alias"), "users_alias");

  // 4. FT.CONFIG GET / SET / HELP
  let timeout = mgr.config_get("TIMEOUT").unwrap();
  assert_eq!(timeout, "500");

  mgr.config_set("TIMEOUT", "1000").unwrap();
  assert_eq!(mgr.config_get("TIMEOUT").unwrap(), "1000");

  let help = mgr.config_help("TIMEOUT").unwrap();
  assert!(help.contains("timeout"));

  // 5. FT.DROPINDEX
  mgr.drop_index("users_idx", false).unwrap();
  assert!(mgr.list_indexes().is_empty());
}

#[test]
fn test_suggestions_dict() {
  let mut dict = SuggestionDict::new();

  // 1. FT.SUGADD
  assert_eq!(
    dict.sug_add("redis", 10.0, false, Some("db".to_string())),
    1
  );
  assert_eq!(dict.sug_add("rediss", 5.0, false, None), 2);
  assert_eq!(dict.sug_add("redigo", 8.0, false, None), 3);
  assert_eq!(
    dict.sug_add("rust", 15.0, false, Some("lang".to_string())),
    4
  );
  assert_eq!(dict.sug_len(), 4);

  // 2. FT.SUGGET prefix
  let res = dict.sug_get("redi", false, true, true, Some(10));
  assert_eq!(res.len(), 3);
  assert_eq!(res[0].string, "redis");
  assert_eq!(res[0].score, 10.0);
  assert_eq!(res[0].payload, Some("db".to_string()));

  // 3. FT.SUGGET fuzzy
  let fuzzy_res = dict.sug_get("radis", true, true, false, Some(5));
  assert!(!fuzzy_res.is_empty());
  assert_eq!(fuzzy_res[0].string, "redis");

  // 4. FT.SUGDEL & SUGLEN
  assert!(dict.sug_del("redigo"));
  assert_eq!(dict.sug_len(), 3);
  assert!(!dict.sug_del("non_existent"));
}

#[test]
fn test_extract_doc_terms_and_schema_types() {
  let schema = SearchIndexSchema::with_full_spec(
    "idx".to_string(),
    IndexOnDataType::Json,
    vec!["user:".to_string()],
    vec![
      IndexField::new("title", IndexFieldType::Text),
      IndexField::with_tag("tags", Some(','), false),
      IndexField::with_numeric("score", true),
      IndexField::with_vector("embedding", 4, DistanceMetric::Cosine),
    ],
  );

  let doc = sonic_rs::json!({
      "title": "Distributed Database in Rust",
      "tags": "db, kv, raft",
      "score": 99.5
  });
  let raw = sonic_rs::to_vec(&doc).unwrap();
  let terms = extract_doc_terms(&schema, "user:1", &raw);

  let term_set: RapidHashSet<(String, String)> = terms.into_iter().collect();
  assert!(term_set.contains(&("title".to_string(), "distributed".to_string())));
  assert!(term_set.contains(&("title".to_string(), "database".to_string())));
  assert!(term_set.contains(&("title".to_string(), "rust".to_string())));
  assert!(term_set.contains(&("tags".to_string(), "db".to_string())));
  assert!(term_set.contains(&("tags".to_string(), "kv".to_string())));
  assert!(term_set.contains(&("tags".to_string(), "raft".to_string())));
  assert!(term_set.contains(&("score".to_string(), encode_sortable_f64(99.5))));

  assert!(schema.matches_key("user:100"));
  assert!(!schema.matches_key("post:100"));

  let field = schema.get_field("tags").unwrap();
  assert_eq!(field.field_type, IndexFieldType::Tag);
}

#[test]
fn test_phrase_and_slop_search() {
  let schema = SearchIndexSchema::with_full_spec(
    "idx_phrases".to_string(),
    IndexOnDataType::Hash,
    vec!["doc:".to_string()],
    vec![IndexField::new("content", IndexFieldType::Text)],
  );

  let mut idx = InvertedIndex::new();

  let doc1 = sonic_rs::json!({ "content": "quick brown fox jumps over lazy dog" });
  let raw1 = sonic_rs::to_vec(&doc1).unwrap();
  idx
    .index_doc(&schema, "doc:1", &raw1, Some(1.0), None)
    .unwrap();

  let doc2 = sonic_rs::json!({ "content": "brown quick jumps dog over lazy" });
  let raw2 = sonic_rs::to_vec(&doc2).unwrap();
  idx
    .index_doc(&schema, "doc:2", &raw2, Some(1.0), None)
    .unwrap();

  // 1. Exact phrase "quick brown"
  let res1 = idx
    .search(&schema, "\"quick brown\"", &FtSearch::default())
    .unwrap();
  assert_eq!(res1.total_results, 1);
  assert_eq!(res1.docs[0].id, "doc:1".to_string());

  // 2. Exact phrase "brown quick"
  let res2 = idx
    .search(&schema, "\"brown quick\"", &FtSearch::default())
    .unwrap();
  assert_eq!(res2.total_results, 1);
  assert_eq!(res2.docs[0].id, "doc:2".to_string());
}

#[test]
fn test_advanced_tag_and_escaping_and_numbers() {
  let schema = SearchIndexSchema::with_full_spec(
    "idx_tags".to_string(),
    IndexOnDataType::Hash,
    vec!["user:".to_string()],
    vec![
      IndexField::with_tag("email_tag", Some(','), false),
      IndexField::with_tag("num_tag", Some(','), false),
    ],
  );

  let mut idx = InvertedIndex::new();

  let doc1 = sonic_rs::json!({
      "email_tag": "test\\@example.com, hello\\ world",
      "num_tag": "3.1415926, 42"
  });
  let raw1 = sonic_rs::to_vec(&doc1).unwrap();
  idx
    .index_doc(&schema, "user:1", &raw1, Some(1.0), None)
    .unwrap();

  // 1. Escaped character query
  let res1 = idx
    .search(
      &schema,
      r"@email_tag:{test\@example\.com}",
      &FtSearch::default(),
    )
    .unwrap();
  assert_eq!(res1.total_results, 1);

  let res2 = idx
    .search(&schema, r"@email_tag:{hello\ world}", &FtSearch::default())
    .unwrap();
  assert_eq!(res2.total_results, 1);

  // 2. Number tag query
  let res3 = idx
    .search(&schema, "@num_tag:{3.1415926}", &FtSearch::default())
    .unwrap();
  assert_eq!(res3.total_results, 1);

  // 3. Prefix tag query
  let res4 = idx
    .search(&schema, "@email_tag:{test*}", &FtSearch::default())
    .unwrap();
  assert_eq!(res4.total_results, 1);
}

#[test]
fn test_hybrid_vector_knn_and_prefilter_search() {
  let schema = SearchIndexSchema::with_full_spec(
    "idx_vectors".to_string(),
    IndexOnDataType::Json,
    vec!["item:".to_string()],
    vec![
      IndexField::with_tag("genre", Some(','), false),
      IndexField::with_numeric("price", true),
      IndexField::with_vector("vec", 3, DistanceMetric::L2),
    ],
  );

  let mut idx = InvertedIndex::new();

  let doc1 = sonic_rs::json!({
      "genre": "scifi",
      "price": 20.0,
      "vec": [1.0, 0.0, 0.0]
  });
  idx
    .index_doc(
      &schema,
      "item:1",
      &sonic_rs::to_vec(&doc1).unwrap(),
      Some(1.0),
      None,
    )
    .unwrap();

  let doc2 = sonic_rs::json!({
      "genre": "fantasy",
      "price": 50.0,
      "vec": [0.0, 1.0, 0.0]
  });
  idx
    .index_doc(
      &schema,
      "item:2",
      &sonic_rs::to_vec(&doc2).unwrap(),
      Some(1.0),
      None,
    )
    .unwrap();

  let doc3 = sonic_rs::json!({
      "genre": "scifi",
      "price": 80.0,
      "vec": [0.9, 0.1, 0.0]
  });
  idx
    .index_doc(
      &schema,
      "item:3",
      &sonic_rs::to_vec(&doc3).unwrap(),
      Some(1.0),
      None,
    )
    .unwrap();

  let mut params = RapidHashMap::default();
  let q_vec = [1.0f64, 0.0, 0.0];
  let bytes: Vec<u8> = q_vec.iter().flat_map(|f| f.to_le_bytes()).collect();
  params.insert("BLOB".to_string(), unsafe {
    String::from_utf8_unchecked(bytes)
  });

  let opts = FtSearch {
    params,
    ..Default::default()
  };

  // 1. Hybrid Search: Prefilter by genre scifi + KNN 1
  let res_hybrid = idx
    .search(&schema, "(@genre:{scifi})=>[KNN 1 @vec $BLOB]", &opts)
    .unwrap();
  assert_eq!(res_hybrid.total_results, 1);
  assert_eq!(res_hybrid.docs[0].id, "item:1".to_string());

  // 2. Vector Range query
  let res_range = idx
    .search(&schema, "@vec:[VECTOR_RANGE 0.5 $BLOB]", &opts)
    .unwrap();
  assert_eq!(res_range.total_results, 2); // item:1 and item:3
}

#[test]
fn test_ft_create_opts_conversion_and_full_lifecycle() {
  let mut mgr = SearchIndexManager::new();

  let create_opts = FtCreate {
    index_name: "articles_idx".to_string(),
    on_data_type: IndexOnDataType::Json,
    prefixes: vec!["article:".to_string()],
    filter: Some("@year > 2020".to_string()),
    default_score: 1.5,
    score_field: Some("score".to_string()),
    payload_field: Some("payload".to_string()),
    language: Some("english".to_string()),
    language_field: None,
    max_text_fields: true,
    no_offsets: false,
    no_hl: false,
    no_fields: false,
    no_freqs: false,
    stop_words: vec!["the".to_string(), "is".to_string()],
    fields: vec![
      IndexField::with_text("title", 2.0, true).with_alias("t"),
      IndexField::with_tag("tags", Some(','), false),
      IndexField::with_numeric("year", true),
      IndexField::with_vector("embedding", 128, DistanceMetric::Cosine),
    ],
  };

  mgr.create_index(create_opts).unwrap();

  let (schema, mut inverted) = mgr.indexes.remove("articles_idx").unwrap();
  let doc = sonic_rs::json!({
      "title": "State of the Art in Vector Search",
      "tags": "db,ai",
      "year": 2024
  });
  inverted
    .index_doc(
      &schema,
      "article:1",
      &sonic_rs::to_vec(&doc).unwrap(),
      Some(1.0),
      None,
    )
    .unwrap();

  let res = inverted
    .search(
      &schema,
      "@tags:{ai} @year:[2020 2025]",
      &FtSearch::default(),
    )
    .unwrap();
  assert_eq!(res.total_results, 1);
  assert_eq!(res.docs[0].id, "article:1".to_string());

  let info = inverted.info(&schema);
  assert_eq!(info.index_name, "articles_idx");
  assert_eq!(info.num_docs, 1);

  mgr
    .indexes
    .insert(HipStr::from("articles_idx"), (schema, inverted));
  let dropped_docs = mgr.drop_index("articles_idx", true).unwrap();
  assert_eq!(dropped_docs, vec!["article:1".to_string()]);
}

#[test]
fn test_hnsw_vector_graph_operations() {
  let mut graph = HnswGraph::new(3, DistanceMetric::L2, 4, 16, 8, 0.01);

  // 1. 插入多个 3D 向量
  let v1 = vec![0.0, 0.0, 0.0];
  let v2 = vec![1.0, 0.0, 0.0];
  let v3 = vec![0.0, 1.0, 0.0];
  let v4 = vec![1.0, 1.0, 0.0];
  let v5 = vec![10.0, 10.0, 10.0];

  graph.insert(HipStr::from("doc1"), v1).unwrap();
  graph.insert(HipStr::from("doc2"), v2).unwrap();
  graph.insert(HipStr::from("doc3"), v3).unwrap();
  graph.insert(HipStr::from("doc4"), v4).unwrap();
  graph.insert(HipStr::from("doc5"), v5).unwrap();

  assert_eq!(graph.nodes.len(), 5);

  // 2. KNN 检索 [0.1, 0.1, 0.0]，Top 2 应该包含 doc1
  let query = vec![0.1, 0.1, 0.0];
  let knn = graph.search_knn(&query, 2, None).unwrap();
  assert_eq!(knn.len(), 2);
  assert_eq!(knn[0].1, "doc1".to_string());

  // 3. VECTOR_RANGE 范围检索：以原点为中心半径 1.5 范围内应有 doc1, doc2, doc3, doc4，不含 doc5
  let range_res = graph.search_range(&[0.0, 0.0, 0.0], 1.5, None).unwrap();
  let range_ids: Vec<String> = range_res
    .into_iter()
    .map(|(_, id)| id.to_string())
    .collect();
  assert!(range_ids.contains(&"doc1".to_string()));
  assert!(range_ids.contains(&"doc2".to_string()));
  assert!(range_ids.contains(&"doc3".to_string()));
  assert!(range_ids.contains(&"doc4".to_string()));
  assert!(!range_ids.contains(&"doc5".to_string()));

  // 4. 删除节点
  assert!(graph.delete("doc1"));
  assert_eq!(graph.nodes.len(), 4);
  let knn_after_del = graph.search_knn(&query, 1, None).unwrap();
  assert_ne!(knn_after_del[0].1, "doc1".to_string());
}

#[test]
fn test_field_grouping_query_parsing() {
  let q = "@title:(rust database)";
  let ast = parse_search_query(q);
  if let SearchQueryNode::And(nodes) = ast {
    assert_eq!(nodes.len(), 2);
    if let SearchQueryNode::Term { field, term, .. } = &nodes[0] {
      assert_eq!(field.as_deref(), Some("title"));
      assert_eq!(term, "rust");
    } else {
      panic!("expected Term node");
    }
    if let SearchQueryNode::Term { field, term, .. } = &nodes[1] {
      assert_eq!(field.as_deref(), Some("title"));
      assert_eq!(term, "database");
    } else {
      panic!("expected Term node");
    }
  } else {
    panic!("expected And node for field grouping");
  }
}

#[test]
fn test_ft_aggregate_groupby_and_reducers() {
  let schema = SearchIndexSchema::with_full_spec(
    "idx_sales".to_string(),
    IndexOnDataType::Json,
    vec!["order:".to_string()],
    vec![
      IndexField::new("region", IndexFieldType::Text),
      IndexField::with_tag("category", Some(','), false),
      IndexField::with_numeric("amount", true),
    ],
  );

  let mut idx = InvertedIndex::new();

  let doc1 = sonic_rs::json!({ "region": "North", "category": "electronics", "amount": 100.0 });
  let doc2 = sonic_rs::json!({ "region": "North", "category": "electronics", "amount": 150.0 });
  let doc3 = sonic_rs::json!({ "region": "North", "category": "books", "amount": 50.0 });
  let doc4 = sonic_rs::json!({ "region": "South", "category": "electronics", "amount": 200.0 });
  let doc5 = sonic_rs::json!({ "region": "South", "category": "books", "amount": 80.0 });

  idx
    .index_doc(
      &schema,
      "order:1",
      &sonic_rs::to_vec(&doc1).unwrap(),
      None,
      None,
    )
    .unwrap();
  idx
    .index_doc(
      &schema,
      "order:2",
      &sonic_rs::to_vec(&doc2).unwrap(),
      None,
      None,
    )
    .unwrap();
  idx
    .index_doc(
      &schema,
      "order:3",
      &sonic_rs::to_vec(&doc3).unwrap(),
      None,
      None,
    )
    .unwrap();
  idx
    .index_doc(
      &schema,
      "order:4",
      &sonic_rs::to_vec(&doc4).unwrap(),
      None,
      None,
    )
    .unwrap();
  idx
    .index_doc(
      &schema,
      "order:5",
      &sonic_rs::to_vec(&doc5).unwrap(),
      None,
      None,
    )
    .unwrap();

  // 1. GROUPBY region REDUCE COUNT AS count REDUCE SUM amount AS total_amount
  let agg_opts = FtAggregate {
    query: "*".to_string(),
    groupbys: vec![FtGroupBy {
      fields: vec!["region".to_string()],
      reducers: vec![
        (FtReducer::Count, Some("order_count".to_string())),
        (
          FtReducer::Sum("amount".to_string()),
          Some("total_amount".to_string()),
        ),
        (
          FtReducer::Avg("amount".to_string()),
          Some("avg_amount".to_string()),
        ),
        (
          FtReducer::Min("amount".to_string()),
          Some("min_amount".to_string()),
        ),
        (
          FtReducer::Max("amount".to_string()),
          Some("max_amount".to_string()),
        ),
        (
          FtReducer::CountDistinct("category".to_string()),
          Some("cat_count".to_string()),
        ),
      ],
    }],
    sortby: vec![("region".to_string(), true)],
    ..Default::default()
  };

  let res = idx.aggregate(&schema, &agg_opts).unwrap();
  assert_eq!(res.total_results, 2);
  assert_eq!(res.rows.len(), 2);

  let row_north = &res.rows[0];
  assert_eq!(
    row_north
      .fields
      .iter()
      .find(|(k, _)| k == "region")
      .map(|(_, v)| v.as_str()),
    Some("North")
  );
  assert_eq!(
    row_north
      .fields
      .iter()
      .find(|(k, _)| k == "order_count")
      .map(|(_, v)| v.as_str()),
    Some("3")
  );
  assert_eq!(
    row_north
      .fields
      .iter()
      .find(|(k, _)| k == "total_amount")
      .map(|(_, v)| v.as_str()),
    Some("300")
  );
  assert_eq!(
    row_north
      .fields
      .iter()
      .find(|(k, _)| k == "avg_amount")
      .map(|(_, v)| v.as_str()),
    Some("100")
  );
  assert_eq!(
    row_north
      .fields
      .iter()
      .find(|(k, _)| k == "min_amount")
      .map(|(_, v)| v.as_str()),
    Some("50")
  );
  assert_eq!(
    row_north
      .fields
      .iter()
      .find(|(k, _)| k == "max_amount")
      .map(|(_, v)| v.as_str()),
    Some("150")
  );
  assert_eq!(
    row_north
      .fields
      .iter()
      .find(|(k, _)| k == "cat_count")
      .map(|(_, v)| v.as_str()),
    Some("2")
  );

  // 2. Manager 路由测试
  let mut mgr = SearchIndexManager::new();
  mgr.create_index(schema).unwrap();
  let mgr_agg = mgr.aggregate("idx_sales", &agg_opts).unwrap();
  assert_eq!(mgr_agg.total_results, 0); // 刚创建为空
}

#[test]
fn test_search_key_binary_encodings_and_field_meta_codecs() {
  // 1. SearchKey 构造测试
  let sk = SearchKey::with_field(0, "idx_test", "title");
  let meta_key = sk.construct_index_meta();
  let prefixes_key = sk.construct_index_prefixes();
  let field_meta_key = sk.construct_field_meta();
  let tag_data_key = sk.construct_tag_field_data("rust", "doc:1");
  let num_data_key = sk.construct_numeric_field_data(42.5, "doc:1");

  assert!(!meta_key.is_empty());
  assert!(!prefixes_key.is_empty());
  assert!(!field_meta_key.is_empty());
  assert!(!tag_data_key.is_empty());
  assert!(!num_data_key.is_empty());

  let meta_begin = sk.construct_all_field_meta_begin();
  let meta_end = sk.construct_all_field_meta_end();
  assert!(meta_begin < meta_end);

  let data_begin = sk.construct_all_field_data_begin();
  let data_end = sk.construct_all_field_data_end();
  assert!(data_begin < data_end);

  // 2. IndexMetadata Codec
  let meta_bytes = encode_index_meta(IndexOnDataType::Hash);
  let decoded_type = decode_index_meta(&meta_bytes).unwrap();
  assert_eq!(decoded_type, IndexOnDataType::Hash);

  let meta_json_bytes = encode_index_meta(IndexOnDataType::Json);
  let decoded_json_type = decode_index_meta(&meta_json_bytes).unwrap();
  assert_eq!(decoded_json_type, IndexOnDataType::Json);

  // 3. IndexPrefixes Codec
  let prefixes = vec!["user:", "customer:"];
  let enc_prefixes = encode_index_prefixes(&prefixes);
  let dec_prefixes = decode_index_prefixes(&enc_prefixes).unwrap();
  assert_eq!(
    dec_prefixes,
    vec!["user:".to_string(), "customer:".to_string()]
  );

  // 4. TagFieldMetadata Codec
  let tag_enc = encode_tag_field_meta(';', true, false);
  let (sep, case_sens, noindex) = decode_tag_field_meta(&tag_enc).unwrap();
  assert_eq!(sep, ';');
  assert!(case_sens);
  assert!(!noindex);

  // 5. NumericFieldMetadata Codec
  let num_enc = encode_numeric_field_meta(true);
  let num_noindex = decode_numeric_field_meta(&num_enc).unwrap();
  assert!(num_noindex);
}

#[test]
fn test_doc_overwrite_and_inverted_index_cleanup() {
  let schema = SearchIndexSchema::with_full_spec(
    "idx_articles".to_string(),
    IndexOnDataType::Json,
    vec!["article:".to_string()],
    vec![
      IndexField::new("title", IndexFieldType::Text),
      IndexField::with_tag("tags", Some(','), false),
      IndexField::with_numeric("views", true),
    ],
  );

  let mut idx = InvertedIndex::new();

  // 第一次写入文章
  let doc_v1 = sonic_rs::json!({
      "title": "Rust and Database Internals",
      "tags": "rust, systems, database",
      "views": 1000.0
  });
  idx
    .index_doc(
      &schema,
      "article:1",
      &sonic_rs::to_vec(&doc_v1).unwrap(),
      None,
      None,
    )
    .unwrap();

  // 检索验证 v1
  let r1 = idx
    .search(&schema, "internals", &FtSearch::default())
    .unwrap();
  assert_eq!(r1.total_results, 1);
  let r1_tag = idx
    .search(&schema, "@tags:{systems}", &FtSearch::default())
    .unwrap();
  assert_eq!(r1_tag.total_results, 1);

  // 第二次更新同一篇文章（覆盖修改）
  let doc_v2 = sonic_rs::json!({
      "title": "Rust Concurrency and Async Performance",
      "tags": "rust, async, concurrency",
      "views": 2500.0
  });
  idx
    .index_doc(
      &schema,
      "article:1",
      &sonic_rs::to_vec(&doc_v2).unwrap(),
      None,
      None,
    )
    .unwrap();

  // 旧词条 "internals" 和 旧标签 "systems" 不应再命中
  let r_old_text = idx
    .search(&schema, "internals", &FtSearch::default())
    .unwrap();
  assert_eq!(r_old_text.total_results, 0);

  let r_old_tag = idx
    .search(&schema, "@tags:{systems}", &FtSearch::default())
    .unwrap();
  assert_eq!(r_old_tag.total_results, 0);

  // 新词条 "concurrency" 和 新标签 "async" 应当命中
  let r_new_text = idx
    .search(&schema, "concurrency", &FtSearch::default())
    .unwrap();
  assert_eq!(r_new_text.total_results, 1);
  assert_eq!(r_new_text.docs[0].id, "article:1".to_string());

  let r_new_tag = idx
    .search(&schema, "@tags:{async}", &FtSearch::default())
    .unwrap();
  assert_eq!(r_new_tag.total_results, 1);

  // 数值范围检索
  let r_old_num = idx
    .search(&schema, "@views:[500 1500]", &FtSearch::default())
    .unwrap();
  assert_eq!(r_old_num.total_results, 0);

  let r_new_num = idx
    .search(&schema, "@views:[2000 3000]", &FtSearch::default())
    .unwrap();
  assert_eq!(r_new_num.total_results, 1);
}

#[test]
fn test_sug_ranking_without_scores() {
  let mut dict = SuggestionDict::new();
  dict.sug_add("redis_search", 10.0, false, None);
  dict.sug_add("redis_core", 50.0, false, None);
  dict.sug_add("redis_raft", 30.0, false, None);
  dict.sug_add("redis_json", 20.0, false, None);

  // 检索 withscores = false
  let res = dict.sug_get("redis_", false, false, false, None);
  assert_eq!(res.len(), 4);
  assert_eq!(res[0].string, "redis_core"); // score 50.0
  assert_eq!(res[1].string, "redis_raft"); // score 30.0
  assert_eq!(res[2].string, "redis_json"); // score 20.0
  assert_eq!(res[3].string, "redis_search"); // score 10.0
  assert_eq!(res[0].score, 0.0); // withscores=false 故返回 0.0

  // 检索 withscores = true
  let res_with_scores = dict.sug_get("redis_", false, true, false, None);
  assert_eq!(res_with_scores[0].string, "redis_core");
  assert_eq!(res_with_scores[0].score, 50.0);
}

#[test]
fn test_binary_vector_with_trailing_whitespace_bytes() {
  // 构造一个浮点数，其最后字节恰好为 0x20 (ASCII Space) 或 0x0A (ASCII Newline)
  // f64::from_bits(0x2000000000000020) 在小端模式下末尾字节为 0x20
  let val_with_space_byte = f64::from_le_bytes([1, 2, 3, 4, 5, 6, 7, 0x20]);
  let vec = vec![1.0, 2.0, val_with_space_byte];
  let bytes: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
  assert_eq!(*bytes.last().unwrap(), 0x20);

  let parsed = parse_vector_from_slice(&bytes, VectorType::Float64).unwrap();
  assert_eq!(parsed, vec);
}

/// 端到端向量与标量（Tag/Numeric/Text）混合多路召回搜索集成测试（对标 RediSearch 向量检索体系）
#[test]
fn test_end_to_end_hybrid_vector_and_scalar_search() {
  let mut idx = InvertedIndex::new();
  let mut schema = SearchIndexSchema::new("products_idx", vec!["product:".to_string()], vec![]);
  schema.on_data_type = IndexOnDataType::Json;
  schema.fields = vec![
    IndexField::with_text("title", 1.0, true),
    IndexField::with_tag("category", Some(','), false),
    IndexField::with_numeric("price", true),
    IndexField::with_vector("embedding", 3, DistanceMetric::L2),
  ];

  // 写入 4 个商品文档
  let doc1 = sonic_rs::json!({
      "title": "MacBook Pro M3 Max",
      "category": "laptop,apple,tech",
      "price": 3499.0,
      "embedding": [1.0, 0.0, 0.0],
  });

  let doc2 = sonic_rs::json!({
      "title": "Dell XPS 16 Developer Edition",
      "category": "laptop,dell,tech",
      "price": 2499.0,
      "embedding": [0.0, 1.0, 0.0],
  });

  let doc3 = sonic_rs::json!({
      "title": "MacBook Air M3 Slim",
      "category": "laptop,apple,budget",
      "price": 1099.0,
      "embedding": [0.9, 0.1, 0.0],
  });

  let doc4 = sonic_rs::json!({
      "title": "Sony Noise Cancelling Headphones",
      "category": "audio,sony,tech",
      "price": 399.0,
      "embedding": [0.0, 0.0, 1.0],
  });

  idx
    .index_doc(
      &schema,
      "product:1",
      &sonic_rs::to_vec(&doc1).unwrap(),
      None,
      None,
    )
    .unwrap();
  idx
    .index_doc(
      &schema,
      "product:2",
      &sonic_rs::to_vec(&doc2).unwrap(),
      None,
      None,
    )
    .unwrap();
  idx
    .index_doc(
      &schema,
      "product:3",
      &sonic_rs::to_vec(&doc3).unwrap(),
      None,
      None,
    )
    .unwrap();
  idx
    .index_doc(
      &schema,
      "product:4",
      &sonic_rs::to_vec(&doc4).unwrap(),
      None,
      None,
    )
    .unwrap();

  // 1. 纯 KNN 检索：查询向量靠近 [1.0, 0.0, 0.0]
  let mut search_opts = FtSearch::default();
  let q_vec = [1.0f64, 0.0, 0.0];
  let q_bytes: Vec<u8> = q_vec.iter().flat_map(|f| f.to_le_bytes()).collect();
  search_opts.params.insert("qvec".to_string(), unsafe {
    String::from_utf8_unchecked(q_bytes.clone())
  });

  let res_knn = idx
    .search(&schema, "*=>[KNN 2 @embedding $qvec]", &search_opts)
    .unwrap();
  assert_eq!(res_knn.total_results, 2);
  let res_ids: Vec<&str> = res_knn.docs.iter().map(|d| d.id.as_str()).collect();
  assert!(res_ids.contains(&"product:1"));
  assert!(res_ids.contains(&"product:3"));

  // 2. 混合过滤：仅筛选 category 为 budget 的商品并在其子集内求 KNN
  let res_hybrid_tag = idx
    .search(
      &schema,
      "(@category:{budget}) => [KNN 2 @embedding $qvec]",
      &search_opts,
    )
    .unwrap();
  assert_eq!(res_hybrid_tag.total_results, 1);
  assert_eq!(res_hybrid_tag.docs[0].id, "product:3");

  // 3. 混合过滤：仅筛选价格 price > 2000 的商品并在其子集内求 KNN
  let res_hybrid_price = idx
    .search(
      &schema,
      "(@price:[2000 4000]) => [KNN 2 @embedding $qvec]",
      &search_opts,
    )
    .unwrap();
  assert_eq!(res_hybrid_price.total_results, 2);
  let price_ids: Vec<&str> = res_hybrid_price
    .docs
    .iter()
    .map(|d| d.id.as_str())
    .collect();
  assert!(price_ids.contains(&"product:1"));
  assert!(price_ids.contains(&"product:2"));

  // 4. 向量范围检索：VECTOR_RANGE 半径 0.2 (仅 product:1 和 product:3 满足 dist <= 0.2)
  let res_range = idx
    .search(&schema, "@embedding:[VECTOR_RANGE 0.2 $qvec]", &search_opts)
    .unwrap();
  assert_eq!(res_range.total_results, 2);
  let range_ids: Vec<&str> = res_range.docs.iter().map(|d| d.id.as_str()).collect();
  assert!(range_ids.contains(&"product:1"));
  assert!(range_ids.contains(&"product:3"));
}
