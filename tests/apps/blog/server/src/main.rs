//! The blog on the Rust server: `blog_server <reze.json> [tower|actix]` prints its origin and serves
//! the pages through the adapter.

mod generated {
    include!(concat!(env!("OUT_DIR"), "/reze_routes.rs"));
}

use std::sync::Arc;

use actix_web::{HttpServer, web};
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

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: blog_server <reze.json> [tower|actix]");
    let manifest = Manifest::load(&path).expect("reze.json is readable");
    let app = routes::App::new().index(home).posts_id(post).manifest(manifest);
    match args.next().as_deref() {
        Some("actix") => actix(app.into()),
        _ => tower(app),
    }
}

#[tokio::main]
async fn tower(app: routes::App<Db>) {
    let router = axum::Router::new().fallback_service(reze_server::tower::service(app, Db::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    axum::serve(listener, router).await.unwrap();
}

fn actix(app: reze_server::App<Db>) {
    let (app, db) = (Arc::new(app), web::Data::new(Db::new()));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    let server = HttpServer::new(move || {
        actix_web::App::new()
            .app_data(db.clone())
            .default_service(reze_server::actix::service(app.clone()))
    });
    actix_web::rt::System::new().block_on(server.listen(listener).unwrap().run()).unwrap();
}
