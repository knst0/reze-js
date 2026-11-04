#[derive(reze::Data)]
pub enum Kind {
    Fruit,
    Vegetable,
}

#[derive(reze::Data)]
pub struct Item {
    pub id: u64,
    pub name: String,
    pub kind: Kind,
    pub price: f64,
    pub note: Option<String>,
}

#[derive(reze::Data)]
pub struct Shop {
    pub title: String,
    pub open: bool,
    pub items: Vec<Item>,
}

pub async fn load(_: Params, _: &Context) -> Result<Shop> {
    unimplemented!()
}
