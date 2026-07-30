pub struct ResearchPlanInput<'a> {
    pub topic: &'a str,
    pub slug: &'a str,
    pub date: &'a str,
}

mod render;

pub use render::render;
