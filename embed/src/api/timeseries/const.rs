pub const ERR_TSDB_KEY_NOT_EXISTS: &str = "ERR TSDB: the key does not exist";
pub const ERR_TSDB_KEY_ALREADY_EXISTS: &str = "ERR TSDB: key already exists";
pub const ERR_TSDB_NOT_TSDB_KEY: &str = "ERR TSDB: the key is not a TSDB key";
pub const ERR_TSDB_TIMESTAMP_OLDER_THAN_RETENTION: &str =
  "ERR TSDB: Timestamp is older than retention";
pub const ERR_TSDB_TIMESTAMP_OLDER_THAN_MAX: &str =
  "ERR TSDB: timestamp must be equal to or higher than the maximum existing timestamp";
pub const ERR_TSDB_DUPLICATE_BLOCK_MODE: &str =
  "ERR TSDB: Error at upsert, update is not supported when DUPLICATE_POLICY is set to BLOCK mode";
pub const ERR_TSDB_SRC_DST_SAME: &str =
  "ERR TSDB: the source key and destination key should be different";
pub const ERR_TSDB_CANNOT_DEL_WITH_RETENTION: &str = "ERR TSDB: When a series has compactions, deleting samples or compaction buckets beyond the series retention period is not possible";
pub const ERR_TSDB_CORRUPTED_DATA_INDEX: &str = "ERR TSDB: corrupted timeseries data index";
pub const ERR_TSDB_CORRUPTED_SRC_META: &str = "ERR TSDB: corrupted source metadata";
pub const ERR_TSDB_CORRUPTED_DST_META: &str = "ERR TSDB: corrupted dest metadata";
pub const ERR_TSDB_SRC_ALREADY_HAS_RULE: &str =
  "ERR TSDB: the source key already has a source rule";
pub const ERR_TSDB_DST_ALREADY_HAS_SRC_RULE: &str =
  "ERR TSDB: the destination key already has a src rule";
pub const ERR_TSDB_DST_ALREADY_HAS_DST_RULE: &str =
  "ERR TSDB: the destination key already has a dst rule";
pub const ERR_TSDB_RULE_NOT_EXISTS: &str = "ERR TSDB: compaction rule does not exist";
