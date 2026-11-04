use std::time::Duration;

use crate::server::models::Status;

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
    pub views: Defer<u32>,
    pub topics: Defer<Vec<String>>,
}

pub async fn load(_: Params, cx: &Context) -> Result<Home> {
    let posts = cx
        .posts
        .lock()
        .map_err(|_| RouteError::internal())?
        .iter()
        .map(|post| Summary { id: post.id, title: post.title.clone(), status: post.status })
        .collect();
    let views = Defer::new(async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        42
    });
    let topics = Defer::new(async {
        tokio::time::sleep(Duration::from_millis(400)).await;
        vec!["intro".to_string(), "rust".to_string()]
    });
    Ok(Home { title: "Blog".into(), posts, views, topics })
}
