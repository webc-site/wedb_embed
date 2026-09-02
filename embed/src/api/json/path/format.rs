use sonic_rs::{JsonContainerTrait, JsonValueMutTrait, JsonValueTrait, Value};

/// JSON Merge Patch (RFC 7396).
/// JSON 合并补丁规范实现（RFC 7396）
pub fn json_merge_patch(target: &mut Value, patch: &Value) {
  if let Some(patch_obj) = patch.as_object() {
    if !target.is_object() {
      *target = sonic_rs::json!({});
    }
    let Some(target_obj) = target.as_object_mut() else {
      return;
    };
    for (k, v) in patch_obj.iter() {
      if v.is_null() {
        target_obj.remove(&k);
      } else {
        let entry = target_obj.entry(k).or_insert(sonic_rs::json!(null));
        json_merge_patch(entry, v);
      }
    }
  } else {
    *target = patch.clone();
  }
}

/// Formats JSON string output with INDENT / NEWLINE / SPACE options.
/// 格式化 JSON 字符串输出（对标 Kvrocks INDENT / NEWLINE / SPACE 选项）
pub fn format_json(
  val: &Value,
  indent: Option<&str>,
  newline: Option<&str>,
  space: Option<&str>,
) -> String {
  if indent.is_none() && newline.is_none() && space.is_none() {
    return sonic_rs::to_string(val).unwrap_or_default();
  }

  let mut out = String::with_capacity(128);
  format_value_recursive(val, 0, indent, newline, space, &mut out);
  out
}

fn format_value_recursive(
  val: &Value,
  depth: usize,
  indent: Option<&str>,
  newline: Option<&str>,
  space: Option<&str>,
  out: &mut String,
) {
  if val.is_null() {
    out.push_str("null");
  } else if let Some(b) = val.as_bool() {
    out.push_str(if b { "true" } else { "false" });
  } else if let Some(i) = val.as_i64() {
    let mut b = itoa::Buffer::new();
    out.push_str(b.format(i));
  } else if let Some(u) = val.as_u64() {
    let mut b = itoa::Buffer::new();
    out.push_str(b.format(u));
  } else if let Some(f) = val.as_f64() {
    let mut b = zmij::Buffer::new();
    out.push_str(b.format(f));
  } else if val.is_str() {
    out.push_str(&sonic_rs::to_string(val).unwrap_or_default());
  } else if let Some(arr) = val.as_array() {
    if arr.is_empty() {
      out.push_str("[]");
      return;
    }
    let nl = newline.unwrap_or("");
    let has_nl = !nl.is_empty();
    let ind = indent.unwrap_or("");

    out.push('[');
    for (i, elem) in arr.iter().enumerate() {
      if i > 0 {
        out.push(',');
      }
      if has_nl {
        out.push_str(nl);
        for _ in 0..=depth {
          out.push_str(ind);
        }
      }
      format_value_recursive(elem, depth + 1, indent, newline, space, out);
    }
    if has_nl {
      out.push_str(nl);
      for _ in 0..depth {
        out.push_str(ind);
      }
    }
    out.push(']');
  } else if let Some(obj) = val.as_object() {
    if obj.is_empty() {
      out.push_str("{}");
      return;
    }
    let nl = newline.unwrap_or("");
    let has_nl = !nl.is_empty();
    let ind = indent.unwrap_or("");
    let colon_sep = if space == Some(" ") { ": " } else { ":" };

    out.push('{');
    for (i, (k, v)) in obj.iter().enumerate() {
      if i > 0 {
        out.push(',');
      }
      if has_nl {
        out.push_str(nl);
        for _ in 0..=depth {
          out.push_str(ind);
        }
      }
      out.push('"');
      out.push_str(k);
      out.push('"');
      out.push_str(colon_sep);
      format_value_recursive(v, depth + 1, indent, newline, space, out);
    }
    if has_nl {
      out.push_str(nl);
      for _ in 0..depth {
        out.push_str(ind);
      }
    }
    out.push('}');
  }
}
