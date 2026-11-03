//! The blog on the Rust server: `blog_server <reze.json>` prints its origin and serves the pages.

mod generated {
    include!(concat!(env!("OUT_DIR"), "/reze_routes.rs"));
}

use blog_types::{Home, Post, Status, Summary};
use generated::routes;
use reze_server::{Manifest, Result, RouteError};

struct Db {
    posts: Vec<Post>,
}

impl Db {
    fn new() -> Self {
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
            };
        Self {
            posts: vec![
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
            ],
        }
    }
}

async fn home(_: routes::index::Params, db: &Db) -> Result<Home> {
    let posts = db
        .posts
        .iter()
        .map(|post| Summary { id: post.id, title: post.title.clone(), status: post.status })
        .collect();
    Ok(Home { title: "Blog".into(), posts })
}

async fn post(params: routes::posts_id::Params, db: &Db) -> Result<Post> {
    let id: u32 = params.id.parse().map_err(|_| RouteError::not_found())?;
    db.posts.iter().find(|post| post.id == id).cloned().ok_or_else(RouteError::not_found)
}

#[tokio::main]
async fn main() {
    let path = std::env::args().nth(1).expect("usage: blog_server <reze.json>");
    let manifest = Manifest::load(&path).expect("reze.json is readable");
    let app = routes::App::new().index(home).posts_id(post).manifest(manifest);
    let router = axum::Router::new().fallback_service(reze_server::tower::service(app, Db::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    axum::serve(listener, router).await.unwrap();
}
