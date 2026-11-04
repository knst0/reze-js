#[derive(reze::Data)]
pub struct Note {
    pub title: String,
}

#[reze::mutation(revalidate = ["/"])]
pub async fn add_note(_: Note, _: &Parts, _: &Context) -> Result<()> {
    Ok(())
}
