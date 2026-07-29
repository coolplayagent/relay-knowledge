pub struct ResearchPlanInput<'a> {
    pub topic: &'a str,
    pub slug: &'a str,
    pub date: &'a str,
}

include!("render.rs");
