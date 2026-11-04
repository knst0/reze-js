use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

#[derive(reze::Data)]
pub struct Page {
    pub published: SystemTime,
    pub editors: BTreeSet<String>,
    pub years: BTreeSet<u16>,
    pub reactions: BTreeMap<String, u32>,
}

pub async fn load(_: Params, _: &Context) -> Result<Page> {
    unimplemented!()
}
