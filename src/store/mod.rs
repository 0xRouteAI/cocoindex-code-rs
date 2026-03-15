use rusqlite::{Connection, params};
use crate::config::Config;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub async fn new(config: &Config) -> anyhow::Result<Self> {
        if let Some(parent) = Path::new(&config.db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Register sqlite-vec as auto extension
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }

        let conn = Connection::open(&config.db_path)?;

        // Create vec0 virtual table with language partition and auxiliary columns
        // Use embedding_dim from config instead of hardcoded 1536
        let create_table_sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS code_chunks_vec USING vec0(
                id TEXT PRIMARY KEY,
                file_path TEXT,
                language TEXT,
                content TEXT,
                start_line INTEGER,
                end_line INTEGER,
                hash TEXT,
                embedding FLOAT[{}],
                +partition_key=language,
                +auxiliary_columns=[file_path, content, start_line, end_line]
            )",
            config.embedding_dim
        );
        conn.execute(&create_table_sql,
            [],
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn))
        })
    }

    pub async fn delete_file_chunks(&self, file_path: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM code_chunks_vec WHERE file_path = ?",
            params![file_path],
        )?;
        Ok(())
    }

    pub async fn save_chunk(
        &self,
        id: &str,
        file_path: &str,
        language: Option<&str>,
        content: &str,
        start_line: usize,
        end_line: usize,
        hash: &str,
        embedding: &[f32],
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();

        let embedding_blob = unsafe {
            std::slice::from_raw_parts(
                embedding.as_ptr() as *const u8,
                embedding.len() * std::mem::size_of::<f32>(),
            )
        };

        // Insert into vec0 virtual table
        conn.execute(
            "INSERT OR REPLACE INTO code_chunks_vec
             (id, file_path, language, content, start_line, end_line, hash, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                file_path,
                language,
                content,
                start_line as i64,
                end_line as i64,
                hash,
                embedding_blob
            ],
        )?;

        Ok(())
    }

    pub async fn get_file_hashes(&self) -> anyhow::Result<HashMap<String, String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT DISTINCT file_path, hash FROM code_chunks_vec")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut hashes = HashMap::new();
        for row in rows {
            let (path, hash) = row?;
            hashes.insert(path, hash);
        }
        Ok(hashes)
    }

    pub async fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        offset: usize,
        languages: Option<&[String]>,
        paths: Option<&[String]>,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let conn = self.conn.lock().unwrap();

        let embedding_blob = unsafe {
            std::slice::from_raw_parts(
                query_embedding.as_ptr() as *const u8,
                query_embedding.len() * std::mem::size_of::<f32>(),
            )
        };

        if paths.is_some_and(|patterns| !patterns.is_empty()) {
            return self.search_full_scan(
                &conn,
                embedding_blob,
                limit,
                offset,
                languages,
                paths,
            );
        }

        // Use partition-aware KNN search when filtering by single language
        if let Some(langs) = languages {
            if langs.len() == 1 {
                // Single language: use partition key for optimal performance
                return self.search_single_language(
                    &conn,
                    embedding_blob,
                    limit,
                    offset,
                    &langs[0],
                    paths,
                );
            } else if !langs.is_empty() {
                // Multiple languages: merge results from each partition
                return self.search_multiple_languages(
                    &conn,
                    embedding_blob,
                    limit,
                    offset,
                    langs,
                    paths,
                );
            }
        }

        // No language filter or path filter: use global KNN search
        self.search_global(&conn, embedding_blob, limit, offset, paths)
    }

    fn search_full_scan(
        &self,
        conn: &Connection,
        embedding_blob: &[u8],
        limit: usize,
        offset: usize,
        languages: Option<&[String]>,
        paths: Option<&[String]>,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let mut sql = String::from(
            "SELECT file_path, language, content, start_line, end_line,
                    vec_distance_L2(embedding, ?) as distance
             FROM code_chunks_vec",
        );

        let mut conditions = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(embedding_blob.to_vec())];

        if let Some(langs) = languages {
            if !langs.is_empty() {
                let placeholders = langs.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                conditions.push(format!("language IN ({})", placeholders));
                for language in langs {
                    params_vec.push(Box::new(language.clone()));
                }
            }
        }

        if let Some(path_patterns) = paths {
            if !path_patterns.is_empty() {
                let path_conditions = path_patterns
                    .iter()
                    .map(|_| "file_path GLOB ?")
                    .collect::<Vec<_>>()
                    .join(" OR ");
                conditions.push(format!("({})", path_conditions));
                for pattern in path_patterns {
                    params_vec.push(Box::new(pattern.clone()));
                }
            }
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        sql.push_str(" ORDER BY distance LIMIT ? OFFSET ?");
        params_vec.push(Box::new(limit as i64));
        params_vec.push(Box::new(offset as i64));

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|value| value.as_ref()).collect();

        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(SearchResult {
                file_path: row.get(0)?,
                language: row.get(1).ok(),
                content: row.get(2)?,
                start_line: row.get::<_, i64>(3)? as usize,
                end_line: row.get::<_, i64>(4)? as usize,
                score: Self::l2_to_cosine(row.get::<_, f32>(5)?),
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn search_single_language(
        &self,
        conn: &Connection,
        embedding_blob: &[u8],
        limit: usize,
        offset: usize,
        language: &str,
        paths: Option<&[String]>,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let mut sql = String::from(
            "SELECT file_path, language, content, start_line, end_line, distance
             FROM code_chunks_vec
             WHERE embedding MATCH ? AND k = ? AND language = ?"
        );

        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(embedding_blob.to_vec()),
            Box::new(limit + offset),
            Box::new(language.to_string()),
        ];

        // Add path filter if specified
        if let Some(path_patterns) = paths {
            if !path_patterns.is_empty() {
                let conditions = path_patterns
                    .iter()
                    .map(|_| "file_path GLOB ?")
                    .collect::<Vec<_>>()
                    .join(" OR ");
                sql.push_str(&format!(" AND ({})", conditions));
                for pattern in path_patterns {
                    params_vec.push(Box::new(pattern.clone()));
                }
            }
        }

        sql.push_str(" ORDER BY distance");

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();

        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(SearchResult {
                file_path: row.get(0)?,
                language: row.get(1).ok(),
                content: row.get(2)?,
                start_line: row.get::<_, i64>(3)? as usize,
                end_line: row.get::<_, i64>(4)? as usize,
                score: Self::l2_to_cosine(row.get::<_, f32>(5)?),
            })
        })?;

        let mut results: Vec<SearchResult> = rows.collect::<Result<Vec<_>, _>>()?;

        // Apply offset
        if offset < results.len() {
            results.drain(0..offset);
        } else {
            results.clear();
        }

        Ok(results)
    }

    fn search_multiple_languages(
        &self,
        conn: &Connection,
        embedding_blob: &[u8],
        limit: usize,
        offset: usize,
        languages: &[String],
        paths: Option<&[String]>,
    ) -> anyhow::Result<Vec<SearchResult>> {
        // Query each language partition separately and merge results
        let mut all_results = Vec::new();

        for lang in languages {
            let results = self.search_single_language(
                conn,
                embedding_blob,
                limit + offset,
                0,
                lang,
                paths,
            )?;
            all_results.extend(results);
        }

        // Sort by score (descending) and apply limit/offset
        all_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        if offset < all_results.len() {
            all_results.drain(0..offset);
        } else {
            all_results.clear();
        }

        all_results.truncate(limit);

        Ok(all_results)
    }

    fn search_global(
        &self,
        conn: &Connection,
        embedding_blob: &[u8],
        limit: usize,
        offset: usize,
        paths: Option<&[String]>,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let mut sql = String::from(
            "SELECT file_path, language, content, start_line, end_line, distance
             FROM code_chunks_vec
             WHERE embedding MATCH ? AND k = ?"
        );

        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(embedding_blob.to_vec()),
            Box::new(limit + offset),
        ];

        // Add path filter if specified
        if let Some(path_patterns) = paths {
            if !path_patterns.is_empty() {
                let conditions = path_patterns
                    .iter()
                    .map(|_| "file_path GLOB ?")
                    .collect::<Vec<_>>()
                    .join(" OR ");
                sql.push_str(&format!(" AND ({})", conditions));
                for pattern in path_patterns {
                    params_vec.push(Box::new(pattern.clone()));
                }
            }
        }

        sql.push_str(" ORDER BY distance");

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();

        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(SearchResult {
                file_path: row.get(0)?,
                language: row.get(1).ok(),
                content: row.get(2)?,
                start_line: row.get::<_, i64>(3)? as usize,
                end_line: row.get::<_, i64>(4)? as usize,
                score: Self::l2_to_cosine(row.get::<_, f32>(5)?),
            })
        })?;

        let mut results: Vec<SearchResult> = rows.collect::<Result<Vec<_>, _>>()?;

        // Apply offset
        if offset < results.len() {
            results.drain(0..offset);
        } else {
            results.clear();
        }

        Ok(results)
    }

    // Convert L2 distance to cosine similarity score
    fn l2_to_cosine(l2_distance: f32) -> f32 {
        1.0 - (l2_distance * l2_distance / 2.0)
    }

    pub async fn clear_all_data(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM code_chunks_vec", [])?;
        Ok(())
    }

    pub async fn get_all_indexed_files(&self) -> anyhow::Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT DISTINCT file_path FROM code_chunks_vec")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
    }

    pub async fn get_stats(&self) -> anyhow::Result<StoreStats> {
        let conn = self.conn.lock().unwrap();

        let total_chunks = conn.query_row(
            "SELECT COUNT(*) FROM code_chunks_vec",
            [],
            |row| row.get::<_, i64>(0),
        )? as usize;

        let total_files = conn.query_row(
            "SELECT COUNT(DISTINCT file_path) FROM code_chunks_vec",
            [],
            |row| row.get::<_, i64>(0),
        )? as usize;

        let mut stmt = conn.prepare(
            "SELECT COALESCE(language, 'unknown'), COUNT(*)
             FROM code_chunks_vec
             GROUP BY COALESCE(language, 'unknown')",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;

        let mut languages = HashMap::new();
        for row in rows {
            let (language, count) = row?;
            languages.insert(language, count);
        }

        Ok(StoreStats {
            total_chunks,
            total_files,
            languages,
        })
    }

    pub async fn delete_files(&self, file_paths: &[String]) -> anyhow::Result<()> {
        if file_paths.is_empty() {
            return Ok(());
        }

        let conn = self.conn.lock().unwrap();
        for file_path in file_paths {
            conn.execute(
                "DELETE FROM code_chunks_vec WHERE file_path = ?",
                params![file_path],
            )?;
        }
        Ok(())
    }

    pub fn clone_internal(&self) -> Self {
        Self { conn: self.conn.clone() }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct SearchResult {
    pub file_path: String,
    pub language: Option<String>,
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
    pub score: f32,
}

#[derive(Debug, serde::Serialize)]
pub struct StoreStats {
    pub total_chunks: usize,
    pub total_files: usize,
    pub languages: HashMap<String, usize>,
}
