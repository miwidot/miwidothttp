use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use memmap2::Mmap;
use std::fs::File;
use std::time::{SystemTime, UNIX_EPOCH};
use bytes::Bytes;
use mime_guess::MimeGuess;
use axum::response::{Response, IntoResponse};
use axum::http::{StatusCode, header};
use axum::body::Body;

#[derive(Clone)]
pub struct CachedFile {
    pub content: Bytes,
    pub mime_type: String,
    pub etag: String,
    pub last_modified: u64,
}

pub struct StaticCache {
    cache: Arc<RwLock<HashMap<PathBuf, Arc<CachedFile>>>>,
    use_mmap: bool,
}

impl StaticCache {
    pub fn new(use_mmap: bool) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            use_mmap,
        }
    }

    pub async fn serve_file(&self, path: &Path) -> Response {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(path) {
                return Self::build_response(cached.clone());
            }
        }

        // Load file
        match self.load_file(path).await {
            Ok(cached_file) => {
                let cached = Arc::new(cached_file);
                
                // Store in cache
                {
                    let mut cache = self.cache.write().await;
                    cache.insert(path.to_path_buf(), cached.clone());
                }
                
                Self::build_response(cached)
            }
            Err(_) => {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("404 Not Found"))
                    .unwrap()
            }
        }
    }

    async fn load_file(&self, path: &Path) -> Result<CachedFile, std::io::Error> {
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        let modified = metadata.modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let content = if self.use_mmap && metadata.len() > 4096 {
            // Use memory mapping for larger files
            unsafe {
                let mmap = Mmap::map(&file)?;
                Bytes::copy_from_slice(&mmap[..])
            }
        } else {
            // Read small files directly
            Bytes::from(std::fs::read(path)?)
        };

        let mime_type = MimeGuess::from_path(path)
            .first_or_octet_stream()
            .to_string();

        // Simple ETag based on size and modification time
        let etag = format!("\"{:x}-{:x}\"", metadata.len(), modified);

        Ok(CachedFile {
            content,
            mime_type,
            etag,
            last_modified: modified,
        })
    }

    fn build_response(cached: Arc<CachedFile>) -> Response {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, &cached.mime_type)
            .header(header::ETAG, &cached.etag)
            .header(header::CACHE_CONTROL, "public, max-age=3600")
            .header("Last-Modified", format!("{}", cached.last_modified))
            .body(Body::from(cached.content.clone()))
            .unwrap()
    }

    pub async fn clear_cache(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}