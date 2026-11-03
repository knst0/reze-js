use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use reze::{Data, Defer};

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
    pub views: Defer<u32>,
    pub topics: Defer<Vec<String>>,
}

#[derive(Clone, Data)]
pub enum Cover {
    Plain,
    Image { src: String, alt: String },
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
    pub published: SystemTime,
    pub reactions: BTreeMap<String, u32>,
    pub editors: BTreeSet<String>,
    pub cover: Cover,
    pub rank: (String, u32),
    pub comments: Vec<Comment>,
}

#[derive(Clone, Data)]
pub struct Comment {
    pub author: String,
    pub body: String,
}

#[derive(Data)]
pub struct NewComment {
    pub post: u32,
    pub author: String,
    pub body: String,
    pub stars: Option<u8>,
}

#[derive(Data)]
pub struct Added {
    pub count: u32,
}
