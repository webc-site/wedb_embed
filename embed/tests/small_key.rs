use std::mem::{align_of, size_of};

use wedb_embed::{INLINE_CAP, SmallKey};

#[ctor::ctor(unsafe)]
fn _log_init() {
  log_init::init();
}

#[test]
fn test_small_key_exact_cache_line_size() {
  assert_eq!(
    size_of::<SmallKey>(),
    64,
    "SmallKey 必须精确占用 64 字节以对齐 CPU 单个 L1 缓存行"
  );
  assert_eq!(
    align_of::<SmallKey>(),
    8,
    "SmallKey 在 64 位平台上必须按 8 字节对齐"
  );
}

#[test]
fn test_small_key_inline_and_heap_boundaries() {
  // 0 长度
  let empty = SmallKey::new();
  assert!(empty.is_empty());
  assert_eq!(empty.len(), 0);
  assert_eq!(empty.as_slice(), b"");

  // 刚好 55 字节（INLINE_CAP）
  let data55 = vec![b'k'; INLINE_CAP];
  let sk55 = SmallKey::from_slice(&data55);
  assert!(matches!(sk55, SmallKey::Inline { .. }));
  assert_eq!(sk55.len(), INLINE_CAP);
  assert_eq!(sk55.as_bytes(), &data55[..]);

  // 56 字节（超出 INLINE_CAP 触发堆分配）
  let data56 = vec![b'k'; INLINE_CAP + 1];
  let sk56 = SmallKey::from_slice(&data56);
  assert!(matches!(sk56, SmallKey::Heap(_)));
  assert_eq!(sk56.len(), INLINE_CAP + 1);
  assert_eq!(sk56.as_bytes(), &data56[..]);

  // 逐步 push 跨越边界
  let mut sk = SmallKey::new();
  for i in 0..INLINE_CAP {
    sk.push(i as u8);
    assert!(matches!(sk, SmallKey::Inline { .. }));
  }
  assert_eq!(sk.len(), INLINE_CAP);
  sk.push(99);
  assert!(matches!(sk, SmallKey::Heap(_)));
  assert_eq!(sk.len(), INLINE_CAP + 1);
  assert_eq!(sk[INLINE_CAP], 99);

  // extend_from_slice 跨越边界
  let mut sk_ext = SmallKey::from_slice(b"prefix");
  sk_ext.extend_from_slice(&[b'x'; 60]);
  assert!(matches!(sk_ext, SmallKey::Heap(_)));
  assert_eq!(sk_ext.len(), 66);
  assert!(sk_ext.starts_with(b"prefix"));

  // into_vec 转换
  let vec_res = sk55.into_vec();
  assert_eq!(vec_res, data55);
}
