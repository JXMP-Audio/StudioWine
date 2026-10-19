//! HTTPS downloads for the wizard: progress, `.part` files, PE validation, cache reuse.
use anyhow::Context;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use ureq::ResponseExt;

/// Only HTTPS, except plain HTTP to the loopback stub in tests.
fn check_scheme(url: &str) -> anyhow::Result<()> {
    if url.starts_with("https://") || (cfg!(test) && url.starts_with("http://127.0.0.1:")) {
        Ok(())
    } else {
        anyhow::bail!("refusing to download over a non-HTTPS connection: {url}")
    }
}

fn content_length<B>(r: &ureq::http::Response<B>) -> Option<u64> {
    r.headers().get("content-length")?.to_str().ok()?.parse().ok()
}

fn file_name(url: &str) -> anyhow::Result<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').next().unwrap_or("");
    anyhow::ensure!(!name.is_empty() && !name.contains(".."), "no file name in {url}");
    Ok(name.to_string())
}

pub struct Http {
    agent: ureq::Agent,
}

impl Http {
    pub fn new() -> Http {
        let config = ureq::Agent::config_builder()
            .max_redirects(10)
            .user_agent("StudioWine/0.1.0")
            .timeout_connect(Some(std::time::Duration::from_secs(30)))
            .build();
        Http { agent: config.into() }
    }

    pub fn get_text(&self, url: &str) -> anyhow::Result<String> {
        check_scheme(url)?;
        let mut r = self.agent.get(url).call().with_context(|| format!("cannot fetch {url}"))?;
        Ok(r.body_mut().read_to_string().with_context(|| format!("cannot read {url}"))?)
    }

    /// The URL a link finally lands on after redirects.
    pub fn resolve_final_url(&self, url: &str) -> anyhow::Result<String> {
        check_scheme(url)?;
        let r = match self.agent.head(url).call() {
            Ok(r) => r,
            Err(_) => self.agent.get(url).call().with_context(|| format!("cannot reach {url}"))?,
        };
        Ok(r.get_uri().to_string())
    }

    /// Downloads a Windows executable into `dest_dir`, named after the last path segment
    /// of the final URL. Reuses a complete earlier download.
    pub fn download_exe(&self, url: &str, dest_dir: &Path, progress: &dyn Fn(u64, Option<u64>)) -> anyhow::Result<PathBuf> {
        self.try_download(url, dest_dir, progress).with_context(|| format!("download of {url} failed"))
    }

    fn try_download(&self, url: &str, dest_dir: &Path, progress: &dyn Fn(u64, Option<u64>)) -> anyhow::Result<PathBuf> {
        check_scheme(url)?;
        std::fs::create_dir_all(dest_dir)?;
        let head = self.agent.head(url).call().ok();
        if let Some(h) = &head {
            let dest = dest_dir.join(file_name(&h.get_uri().to_string())?);
            if let (Some(len), Ok(meta)) = (content_length(h), std::fs::metadata(&dest)) {
                if meta.len() == len {
                    progress(len, Some(len));
                    return Ok(dest);
                }
            }
        }
        let mut resp = self.agent.get(url).call()?;
        let final_url = resp.get_uri().to_string();
        check_scheme(&final_url)?;
        let dest = dest_dir.join(file_name(&final_url)?);
        let total = content_length(&resp);
        let part = dest.with_file_name(format!("{}.part", dest.file_name().unwrap().to_string_lossy()));
        let result = (|| -> anyhow::Result<()> {
            let mut reader = resp.body_mut().as_reader();
            let mut out = std::fs::File::create(&part)?;
            let mut buf = vec![0u8; 256 * 1024];
            let mut done: u64 = 0;
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                if done == 0 && !buf[..n].starts_with(b"MZ") {
                    anyhow::bail!("the server did not send a Windows program (got {:?}…)", String::from_utf8_lossy(&buf[..n.min(16)]));
                }
                out.write_all(&buf[..n])?;
                done += n as u64;
                progress(done, total);
            }
            if let Some(t) = total {
                anyhow::ensure!(done == t, "connection closed after {done} of {t} bytes");
            }
            anyhow::ensure!(done >= 2, "empty download");
            out.sync_all()?;
            Ok(())
        })();
        if let Err(e) = result {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }
        std::fs::rename(&part, &dest)?;
        Ok(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Serves `/file.exe` with `body`, declaring `declared_len` bytes; `/go` redirects to it.
    /// Returns (base url, number of GET requests for /file.exe).
    fn stub(body: Vec<u8>, declared_len: usize) -> (String, Arc<AtomicUsize>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let gets = Arc::new(AtomicUsize::new(0));
        let g = gets.clone();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let mut s = s.unwrap();
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = req.lines().next().unwrap_or("").to_string();
                if line.contains(" /go ") {
                    let _ = s.write_all(b"HTTP/1.1 302 Found\r\nLocation: /file.exe\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                } else if line.starts_with("HEAD") {
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {declared_len}\r\nConnection: close\r\n\r\n");
                } else {
                    g.fetch_add(1, Ordering::SeqCst);
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {declared_len}\r\nConnection: close\r\n\r\n");
                    let _ = s.write_all(&body);
                }
            }
        });
        (base, gets)
    }

    fn pe(n: usize) -> Vec<u8> {
        let mut b = b"MZ".to_vec();
        b.resize(n, 7);
        b
    }

    #[test]
    fn downloads_and_renames() {
        let (base, _) = stub(pe(3000), 3000);
        let d = tempfile::tempdir().unwrap();
        let f = Http::new().download_exe(&format!("{base}/go"), d.path(), &|_, _| {}).unwrap();
        assert_eq!(f, d.path().join("file.exe"));
        assert_eq!(std::fs::read(&f).unwrap().len(), 3000);
        assert!(!d.path().join("file.exe.part").exists());
    }

    #[test]
    fn rejects_non_pe_payload() {
        let (base, _) = stub(b"<html>moved</html>".to_vec(), 18);
        let d = tempfile::tempdir().unwrap();
        let e = Http::new().download_exe(&format!("{base}/go"), d.path(), &|_, _| {}).unwrap_err();
        assert!(format!("{e:#}").contains("/go"), "{e:#}");
        assert!(!d.path().join("file.exe").exists());
    }

    #[test]
    fn interrupted_download_leaves_no_final_file() {
        let (base, _) = stub(pe(500), 1000);
        let d = tempfile::tempdir().unwrap();
        assert!(Http::new().download_exe(&format!("{base}/go"), d.path(), &|_, _| {}).is_err());
        assert!(!d.path().join("file.exe").exists());
    }

    #[test]
    fn reuses_complete_cached_file() {
        let (base, gets) = stub(pe(2000), 2000);
        let d = tempfile::tempdir().unwrap();
        let h = Http::new();
        h.download_exe(&format!("{base}/go"), d.path(), &|_, _| {}).unwrap();
        h.download_exe(&format!("{base}/go"), d.path(), &|_, _| {}).unwrap();
        assert_eq!(gets.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn progress_reports_total() {
        let (base, _) = stub(pe(5000), 5000);
        let d = tempfile::tempdir().unwrap();
        let last = std::sync::Mutex::new((0u64, None));
        Http::new().download_exe(&format!("{base}/go"), d.path(), &|a, b| *last.lock().unwrap() = (a, b)).unwrap();
        assert_eq!(*last.lock().unwrap(), (5000, Some(5000)));
    }

    #[test]
    fn plain_http_rejected_outside_loopback() {
        let d = tempfile::tempdir().unwrap();
        let e = Http::new().download_exe("http://example.com/x.exe", d.path(), &|_, _| {}).unwrap_err();
        assert!(format!("{e:#}").contains("HTTPS"), "{e:#}");
    }
}
