#[derive(reze::Data)]
pub struct Page {
    pub title: String,
}

pub async fn guard(_: Params, _: &Parts, _: &Context) -> Result<()> {
    Ok(())
}

pub async fn load(_: Params, _: &Context) -> Result<Page> {
    unimplemented!()
}
