/// SSCAN return result type definition: (next_cursor, members).
/// SSCAN 返回结果类型定义：(next_cursor, members)
pub type SetScanResult = (u64, Vec<Vec<u8>>);

/// SSCAN_BY_MEMBER return result type definition: (next_cursor_member, members).
/// 基于成员寻址的 SSCAN 返回结果类型定义：(next_cursor_member, members)
pub type SetScanByMemberResult = (Option<Vec<u8>>, Vec<Vec<u8>>);
