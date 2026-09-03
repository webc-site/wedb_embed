pub const ERR_NEW_OBJECTS_MUST_BE_CREATED_AT_ROOT: &str = "new objects must be created at the root";
pub const ERR_INPUT_SHOULD_BE_NUMBER: &str = "the input value should be a number";
pub const ERR_RESULT_IS_INFINITE: &str = "the result is an infinite number";
pub const ERR_STRAPPEND_NEED_STRING: &str = "STRAPPEND need input a string to append";
pub const ERR_JSON_STORAGE_FORMAT_NOT_SUPPORTED: &str = "JSON storage format not supported";
pub const ERR_CORRUPTED_JSON: &str = "ERR corrupted JSON";
pub const ERR_INVALID_JSON: &str = "ERR invalid JSON";
pub const ERR_INVALID_JSON_VALUE: &str = "ERR invalid JSON value";
pub const ERR_INVALID_JSON_NEEDLE: &str = "ERR invalid JSON needle";
pub const ERR_INVALID_JSON_PATCH: &str = "ERR invalid JSON patch";
pub const ERR_PARENT_PATH_DOES_NOT_EXIST: &str =
  "Target path does not exist and parent cannot be found";
pub const ERR_TARGET_PARENT_NOT_OBJECT: &str = "Target parent is not a JSON object to insert into";
pub const ERR_ONLY_ALL_SPACE_INDENT_SUPPORTED: &str =
  "Currently only all-space INDENT is supported";
pub const ERR_ONLY_SPACE_SUPPORTED: &str = "Currently only SPACE ' ' is supported";

/// JSON root path string constant "$".
/// JSON 根路径常量
pub const JSON_ROOT_PATH: &str = "$";

/// Default maximum nesting depth for JSON structures aligned with Kvrocks (1024).
/// JSON 默认最大嵌套深度（对标 Kvrocks default_max_nesting_depth = 1024）
pub const DEFAULT_MAX_NESTING_DEPTH: usize = 1024;
