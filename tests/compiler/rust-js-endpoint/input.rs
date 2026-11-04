#[derive(reze::Data)]
pub struct Page {
    pub title: String,
}

pub async fn load(_: Params, _: &Context) -> Result<Page> {
    Ok(Page { title: String::from("Home") })
}
