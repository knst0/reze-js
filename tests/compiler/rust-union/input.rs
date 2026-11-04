use std::time::SystemTime;

#[derive(reze::Data)]
pub enum Cover {
    Plain,
    Image { src: String, width: u32 },
}

#[derive(reze::Data)]
pub struct Page {
    pub cover: Cover,
    pub rank: (String, u32),
    pub published: SystemTime,
}

pub async fn load(_: Params, _: &Context) -> Result<Page> {
    unimplemented!()
}
