pub mod models;

use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

use models::{Cover, Post, Status};

pub struct Context {
    pub posts: Mutex<Vec<Post>>,
}

pub fn context() -> Context {
    let post =
        |id, title: &str, subtitle: Option<&str>, status, tags: &[&str], views, rating| Post {
            id,
            title: title.into(),
            subtitle: subtitle.map(Into::into),
            body: format!("The body of {title} <with markup> & more."),
            status,
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            likes: id * 2,
            views,
            rating,
            published: UNIX_EPOCH + Duration::from_millis(1_714_564_800_000 + u64::from(id)),
            reactions: [("like".to_string(), id * 3), ("wow".to_string(), 1)].into(),
            editors: ["ann".to_string(), "bo".to_string()].into(),
            cover: if id == 1 {
                Cover::Image { src: "/hello.png".into(), alt: "A wave".into() }
            } else {
                Cover::Plain
            },
            rank: ("weekly".into(), id),
            comments: Vec::new(),
        };
    Context {
        posts: Mutex::new(vec![
            post(
                1,
                "Hello, world",
                Some("The first post"),
                Status::Published,
                &["intro", "rust"],
                12_345_678_901_234,
                4.5,
            ),
            post(2, "Drafts \"quoted\"", None, Status::Draft, &[], 0, 0.1 + 0.2),
        ]),
    }
}
