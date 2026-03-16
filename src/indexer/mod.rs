use std::path::Path;
use walkdir::WalkDir;
use crate::store::Store;
use crate::provider::Provider;
use crate::project::scoped_chunk_id;
use crate::config::ProjectSettings;
use crate::utils::{detect_language_with_overrides, PatternMatcher};
use cocoindex_ops_text::split::{RecursiveChunker, RecursiveSplitConfig, RecursiveChunkConfig};
use std::fs;

pub struct Indexer {
    store: Store,
    provider: Provider,
    settings: ProjectSettings,
    matcher: PatternMatcher,
}

impl Indexer {
    pub fn new(store: Store, provider: Provider, project_root: &Path) -> anyhow::Result<Self> {
        let settings = ProjectSettings::load(project_root)?;
        let matcher = PatternMatcher::new(&settings.include_patterns, &settings.exclude_patterns)?;

        Ok(Self {
            store,
            provider,
            settings,
            matcher,
        })
    }

    pub async fn index_directory(&self, root: &Path) -> anyhow::Result<()> {
        self.index_directory_with_refresh(root, false).await
    }

    pub async fn index_directory_with_refresh(&self, root: &Path, refresh: bool) -> anyhow::Result<()> {
        // Clear all data if refresh is requested
        if refresh {
            println!("Clearing existing index for full refresh...");
            self.store.clear_all_data().await?;
        }

        let existing_hashes = self.store.get_file_hashes().await?;
        let chunker = RecursiveChunker::new(RecursiveSplitConfig::default())
            .map_err(|e: String| anyhow::anyhow!(e))?;

        // Collect all current disk files
        let mut current_files = std::collections::HashSet::new();

        for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();

            // Use pattern matcher instead of hardcoded rules
            let rel_path = path.strip_prefix(root).unwrap_or(path);
            if !self.matcher.matches(rel_path) {
                continue;
            }

            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let hash = format!("{:x}", md5::compute(&content));

            let rel_path_str = rel_path.to_string_lossy().to_string();
            current_files.insert(rel_path_str.clone());

            if let Some(existing_hash) = existing_hashes.get(&rel_path_str) {
                if existing_hash == &hash {
                    continue;
                }
            }

            eprintln!("Indexing: {}", rel_path_str);

            // Delete old chunks for this file before inserting new ones
            self.store.delete_file_chunks(&rel_path_str).await?;

            // Detect language with overrides
            let language = detect_language_with_overrides(path, &self.settings.language_overrides);

            let chunking = self.provider.chunking_profile();
            let config = RecursiveChunkConfig {
                chunk_size: chunking.chunk_size,
                min_chunk_size: Some(chunking.min_chunk_size),
                chunk_overlap: Some(chunking.chunk_overlap),
                language: language.clone(),
            };
            let chunks = chunker.split(&content, config);

            let texts: Vec<String> = chunks.iter()
                .map(|c| content[c.range.start..c.range.end].to_string())
                .collect();

            if texts.is_empty() {
                continue;
            }

            let embeddings = self.provider.get_embeddings(texts).await?;

            for (i, (chunk, embedding)) in chunks.into_iter().zip(embeddings.into_iter()).enumerate() {
                let id = scoped_chunk_id(root, &rel_path_str, i);
                self.store.save_chunk(
                    &id,
                    &rel_path_str,
                    language.as_deref(),
                    chunk.start.line as usize,
                    chunk.end.line as usize,
                    &hash,
                    &embedding,
                ).await?;
            }
        }

        // Clean up deleted files from DB
        let indexed_files = self.store.get_all_indexed_files().await?;
        let deleted_files: Vec<String> = indexed_files
            .into_iter()
            .filter(|f| !current_files.contains(f))
            .collect();

        if !deleted_files.is_empty() {
            eprintln!("Cleaning up {} deleted file(s)...", deleted_files.len());
            self.store.delete_files(&deleted_files).await?;
        }

        Ok(())
    }
}
