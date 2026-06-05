pub(in crate::code) struct GitlinkPathSelector<'a> {
    include_path: &'a dyn Fn(&str) -> bool,
    scope_overlaps: &'a dyn Fn(&str) -> bool,
    child_filters: &'a dyn Fn(&str) -> Option<Vec<String>>,
}

impl<'a> GitlinkPathSelector<'a> {
    pub(in crate::code) fn new(
        include_path: &'a dyn Fn(&str) -> bool,
        scope_overlaps: &'a dyn Fn(&str) -> bool,
    ) -> Self {
        Self::new_with_child_filters(include_path, scope_overlaps, &all_child_paths)
    }

    pub(in crate::code) fn new_with_child_filters(
        include_path: &'a dyn Fn(&str) -> bool,
        scope_overlaps: &'a dyn Fn(&str) -> bool,
        child_filters: &'a dyn Fn(&str) -> Option<Vec<String>>,
    ) -> Self {
        Self {
            include_path,
            scope_overlaps,
            child_filters,
        }
    }

    #[cfg(test)]
    pub(in crate::code) fn all() -> Self {
        Self {
            include_path: &path_always_matches,
            scope_overlaps: &path_always_matches,
            child_filters: &all_child_paths,
        }
    }

    pub(in crate::code) fn includes(&self, path: &str) -> bool {
        (self.include_path)(path)
    }

    pub(in crate::code) fn overlaps(&self, path: &str) -> bool {
        (self.scope_overlaps)(path)
    }

    pub(in crate::code) fn child_filters(&self, path: &str) -> Option<Vec<String>> {
        (self.child_filters)(path)
    }
}

#[cfg(test)]
fn path_always_matches(_: &str) -> bool {
    true
}

fn all_child_paths(_: &str) -> Option<Vec<String>> {
    Some(Vec::new())
}
