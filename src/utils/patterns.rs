use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;
use anyhow::Result;

pub struct PatternMatcher {
    include: GlobSet,
    exclude: GlobSet,
}

impl PatternMatcher {
    pub fn new(
        include_patterns: &[String],
        exclude_patterns: &[String],
    ) -> Result<Self> {
        let mut include_builder = GlobSetBuilder::new();
        for pattern in include_patterns {
            include_builder.add(Glob::new(pattern)?);
        }

        let mut exclude_builder = GlobSetBuilder::new();
        for pattern in exclude_patterns {
            exclude_builder.add(Glob::new(pattern)?);
        }

        Ok(Self {
            include: include_builder.build()?,
            exclude: exclude_builder.build()?,
        })
    }

    pub fn matches(&self, path: &Path) -> bool {
        if self.matches_exclude(path) {
            return false;
        }

        // Then check include rules
        let path_str = path.to_string_lossy();
        self.include.is_match(&*path_str)
    }

    fn matches_exclude(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        if self.exclude.is_match(&*path_str) {
            return true;
        }

        for ancestor in path.ancestors().skip(1) {
            let ancestor_str = ancestor.to_string_lossy();
            if self.exclude.is_match(&*ancestor_str) {
                return true;
            }
        }

        false
    }
}
