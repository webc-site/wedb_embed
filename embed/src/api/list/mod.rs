pub mod r#const;
pub mod r#impl;
pub mod key;
pub mod meta;
pub mod r#move;
pub mod mutate;
pub mod opt;
pub mod pos;

pub use r#const::{ERR_INDEX_OUT_OF_RANGE, ERR_NO_SUCH_KEY, ERR_RANK_ZERO, ERR_WRONG_TYPE};
pub use r#impl::prepare_list_meta_for_write;
pub use key::{
  ItemKeyComposer as ListItemKeyComposer, item as compose_list_item, meta as compose_list_meta_key,
  prefix as compose_list_prefix, prefix_stack as compose_list_prefix_stack,
};
pub use meta::ListMeta;
pub use opt::LPos;

pub type ListPopResult = (Vec<u8>, Vec<Vec<u8>>);
