//! Project-wide unrolling, chunk manipulation, and dispatch helper macros.
//! 全局通用的循环展开、数组构造、内存写入与分发辅助宏

/// Constructs an 8-element array by applying an expression to index 0..8.
/// 通过对索引 0..8 分别求值构造 8 元素数组（完全内联，消除手动 +1, +2, +3）
#[macro_export]
macro_rules! arr_8 {
  ($idx:ident => $expr:expr) => {
    [
      {
        let $idx = 0;
        $expr
      },
      {
        let $idx = 1;
        $expr
      },
      {
        let $idx = 2;
        $expr
      },
      {
        let $idx = 3;
        $expr
      },
      {
        let $idx = 4;
        $expr
      },
      {
        let $idx = 5;
        $expr
      },
      {
        let $idx = 6;
        $expr
      },
      {
        let $idx = 7;
        $expr
      },
    ]
  };
}

/// Unrolls a block 8 times with `$idx` bound to 0..8.
/// 将逻辑按索引 0..8 重复展开 8 次顺序执行
#[macro_export]
macro_rules! unroll_8 {
  ($idx:ident => $expr:expr) => {{
    let $idx = 0;
    $expr;
    let $idx = 1;
    $expr;
    let $idx = 2;
    $expr;
    let $idx = 3;
    $expr;
    let $idx = 4;
    $expr;
    let $idx = 5;
    $expr;
    let $idx = 6;
    $expr;
    let $idx = 7;
    $expr;
  }};
}

/// Writes 8 evaluated elements to consecutive raw pointer memory `*($dst).add(k) = expr(k)`.
/// 向连续裸指针内存顺序写入 8 个计算结果（局部绑定 base 指针，杜绝表达式重复求值）
#[macro_export]
macro_rules! write_8 {
  ($dst:expr, $idx:ident => $expr:expr) => {{
    let dst = $dst;
    $crate::unroll_8!($idx => {
      *dst.add($idx) = $expr;
    });
  }};
}

/// Writes 4 evaluated elements to consecutive raw pointer memory `*($dst).add(k) = expr(k)`.
/// 向连续裸指针内存顺序写入 4 个计算结果（局部绑定 base 指针）
#[macro_export]
macro_rules! write_4 {
  ($dst:expr, $idx:ident => $expr:expr) => {{
    let dst = $dst;
    let $idx = 0;
    *dst.add($idx) = $expr;
    let $idx = 1;
    *dst.add($idx) = $expr;
    let $idx = 2;
    *dst.add($idx) = $expr;
    let $idx = 3;
    *dst.add($idx) = $expr;
  }};
}

/// Dispatches bit-width 1..=20, 24, 28, 32 to monomorphized chunk packing function.
/// 统一分发 1..=20, 24, 28, 32 位宽至单态化 8 元素块打包内核
#[macro_export]
macro_rules! match_pack_23 {
  ($bw:expr, fallback => $fallback:expr, |$w:ident| $arm:expr) => {
    match $bw {
      1 => {
        const $w: u8 = 1;
        $arm
      }
      2 => {
        const $w: u8 = 2;
        $arm
      }
      3 => {
        const $w: u8 = 3;
        $arm
      }
      4 => {
        const $w: u8 = 4;
        $arm
      }
      5 => {
        const $w: u8 = 5;
        $arm
      }
      6 => {
        const $w: u8 = 6;
        $arm
      }
      7 => {
        const $w: u8 = 7;
        $arm
      }
      8 => {
        const $w: u8 = 8;
        $arm
      }
      9 => {
        const $w: u8 = 9;
        $arm
      }
      10 => {
        const $w: u8 = 10;
        $arm
      }
      11 => {
        const $w: u8 = 11;
        $arm
      }
      12 => {
        const $w: u8 = 12;
        $arm
      }
      13 => {
        const $w: u8 = 13;
        $arm
      }
      14 => {
        const $w: u8 = 14;
        $arm
      }
      15 => {
        const $w: u8 = 15;
        $arm
      }
      16 => {
        const $w: u8 = 16;
        $arm
      }
      17 => {
        const $w: u8 = 17;
        $arm
      }
      18 => {
        const $w: u8 = 18;
        $arm
      }
      19 => {
        const $w: u8 = 19;
        $arm
      }
      20 => {
        const $w: u8 = 20;
        $arm
      }
      24 => {
        const $w: u8 = 24;
        $arm
      }
      28 => {
        const $w: u8 = 28;
        $arm
      }
      32 => {
        const $w: u8 = 32;
        $arm
      }
      _ => $fallback,
    }
  };
}
