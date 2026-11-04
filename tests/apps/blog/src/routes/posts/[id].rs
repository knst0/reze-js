use crate::server::models::{Comment, Post};

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

pub async fn load(params: Params, cx: &Context) -> Result<Post> {
    if params.id == "first" {
        return Err(RouteError::redirect("/posts/1"));
    }
    let id: u32 = params.id.parse().map_err(|_| RouteError::not_found())?;
    let posts = cx.posts.lock().map_err(|_| RouteError::internal())?;
    posts.iter().find(|post| post.id == id).cloned().ok_or_else(RouteError::not_found)
}

pub async fn guard(params: Params, request: &Parts, _: &Context) -> Result<()> {
    if params.id != "members" {
        return Ok(());
    }
    if request.uri.query().is_some_and(|query| query.contains("login")) {
        return Err(RouteError::redirect("/"));
    }
    match request.headers.get("x-member") {
        Some(_) => Err(RouteError::not_found()),
        None => Err(RouteError::new(403, "Members only")),
    }
}

#[reze::mutation(revalidate = ["/posts/:id"])]
pub async fn add_comment(input: NewComment, _: &Parts, cx: &Context) -> Result<Added> {
    if input.body == "home" {
        return Err(RouteError::redirect("/"));
    }
    if input.author.is_empty() {
        return Err(RouteError::new(400, "Comments need an author"));
    }
    let mut posts = cx.posts.lock().map_err(|_| RouteError::internal())?;
    let post =
        posts.iter_mut().find(|post| post.id == input.post).ok_or_else(RouteError::not_found)?;
    let stars = "*".repeat(input.stars.unwrap_or(0).into());
    post.comments.push(Comment { author: input.author, body: format!("{}{stars}", input.body) });
    Ok(Added { count: post.comments.len() as u32 })
}
