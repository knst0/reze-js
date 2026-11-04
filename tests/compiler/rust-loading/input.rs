#[derive(reze::Data)]
pub struct Page {
    pub title: String,
    pub count: Defer<u32>,
}

pub async fn load(_: Params, _: &Context) -> Result<Page> {
    unimplemented!()
}
