pub(super) struct GitlinkPathSelector<'a> {
    include_path: &'a dyn Fn(&str) -> bool,
    scope_overlaps: &'a dyn Fn(&str) -> bool,
}

impl<'a> GitlinkPathSelector<'a> {
    pub(super) fn new(
        include_path: &'a dyn Fn(&str) -> bool,
        scope_overlaps: &'a dyn Fn(&str) -> bool,
    ) -> Self {
        Self {
            include_path,
            scope_overlaps,
        }
    }

    #[cfg(test)]
    pub(super) fn all() -> Self {
        Self {
            include_path: &path_always_matches,
            scope_overlaps: &path_always_matches,
        }
    }

    pub(super) fn includes(&self, path: &str) -> bool {
        (self.include_path)(path)
    }

    pub(super) fn overlaps(&self, path: &str) -> bool {
        (self.scope_overlaps)(path)
    }
}

#[cfg(test)]
fn path_always_matches(_: &str) -> bool {
    true
}
