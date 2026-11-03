use reze::Data;

#[derive(Clone, Copy, Data)]
pub enum Status {
    Draft,
    Published,
}

#[derive(Clone, Data)]
pub struct Summary {
    pub id: u32,
    pub title: String,
    pub status: Status,
}

#[derive(Data)]
pub struct Home {
    pub title: String,
    pub posts: Vec<Summary>,
}

#[derive(Clone, Data)]
pub struct Post {
    pub id: u32,
    pub title: String,
    pub subtitle: Option<String>,
    pub body: String,
    pub status: Status,
    pub tags: Vec<String>,
    pub likes: u32,
    pub views: u64,
    pub rating: f64,
}
