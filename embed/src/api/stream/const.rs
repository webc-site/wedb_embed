pub const ERR_STREAM_NOT_FOUND: &str = "Stream not found";
pub const ERR_GROUP_NOT_FOUND: &str = "Consumer group not found";
pub const ERR_GROUP_BUSY: &str = "BUSYGROUP Consumer Group name already exists";
pub const ERR_XGROUP_KEY_REQUIRE_EXIST: &str = "The XGROUP subcmd requires the key to exist. Note that for CREATE you may want to use the MKSTREAM option to create an empty stream automatically.";
pub const ERR_XGROUP_KEY_MUST_EXIST: &str = "The XGROUP subcmd requires the key to exist.";
pub const ERR_XGROUP_KEY_GROUP_MUST_EXIST: &str =
  "The XGROUP subcmd requires the key and group to exist.";
pub const ERR_XGROUP_GROUP_MUST_EXIST: &str = "The XGROUP subcmd requires the group to exist.";
pub const ERR_INVALID_START_ID_INTERVAL: &str = "invalid start ID for the interval";
pub const ERR_INVALID_END_ID_INTERVAL: &str = "invalid end ID for the interval";
pub const ERR_SET_ID_SMALLER_THAN_TOP: &str =
  "The ID specified in XSETID is smaller than the target stream top item";
pub const ERR_SET_ID_ENTRIES_ADDED_SMALLER: &str =
  "The entries_added specified in XSETID is smaller than the target stream length";
pub const ERR_SET_ID_MAX_DEL_GREATER: &str =
  "The ID specified in XSETID is smaller than the provided max_deleted_entry_id";
pub const ERR_EMPTY_STREAM_ENTRIES_ADDED: &str =
  "an empty stream should have non-zero value of ENTRIESADDED";
pub const ERR_EMPTY_STREAM_MAX_DELETED: &str = "an empty stream should have MAXDELETEDID";
