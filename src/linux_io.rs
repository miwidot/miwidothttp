// Linux-specific io_uring optimizations
#[cfg(target_os = "linux")]
use tokio_uring::fs::File;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use bytes::Bytes;

#[cfg(target_os = "linux")]
pub async fn read_file_uring(path: &Path) -> Result<Bytes, std::io::Error> {
    let file = File::open(path).await?;
    let mut buf = Vec::new();
    
    // Read entire file using io_uring
    let (res, buf_returned) = file.read_at(buf, 0).await;
    let n = res?;
    buf = buf_returned;
    buf.truncate(n);
    
    Ok(Bytes::from(buf))
}

#[cfg(not(target_os = "linux"))]
pub async fn read_file_uring(path: &std::path::Path) -> Result<bytes::Bytes, std::io::Error> {
    // Fallback to standard tokio fs for non-Linux
    use tokio::fs;
    let content = fs::read(path).await?;
    Ok(bytes::Bytes::from(content))
}

// Check if io_uring is available at runtime
pub fn is_uring_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Try to probe io_uring support
        match io_uring::IoUring::new(2) {
            Ok(_) => true,
            Err(_) => false,
        }
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}