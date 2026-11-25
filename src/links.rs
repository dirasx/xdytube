//! A single running instance: a second launch (e.g. with a YouTube link on the
//! command line) hands its argument to the first and exits.
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// Turns a `streamtube:` link back into the YouTube URL it wraps.
/// Accepts `xdytube://open?url=<encoded>`, `xdytube:https://…` and
/// `xdytube://www.youtube.com/watch?v=…`; anything else is returned unchanged.
pub fn normalize(arg: &str) -> String {
    let a = arg.trim().trim_matches('"');
    let Some(rest) = a.strip_prefix("xdytube:").or_else(|| a.strip_prefix("XDYTUBE:")).or_else(|| a.strip_prefix("streamtube:")).or_else(|| a.strip_prefix("STREAMTUBE:")) else {
        return a.to_string();
    };
    let rest = rest.trim_start_matches('/');
    if let Some(q) = rest.strip_prefix("open?").or_else(|| rest.strip_prefix("open/?")) {
        if let Some(u) = q.split('&').find_map(|kv| kv.strip_prefix("url=")) {
            return crate::api::http::urldecode(u);
        }
        if let Some(v) = q.split('&').find_map(|kv| kv.strip_prefix("v=")) {
            return format!("https://www.youtube.com/watch?v={v}");
        }
    }
    if rest.starts_with("http:") || rest.starts_with("https:") {
        // "https:/www…" when a browser collapsed the slashes
        let (scheme, tail) = rest.split_once(':').unwrap_or(("https", ""));
        return format!("{scheme}://{}", tail.trim_start_matches('/'));
    }
    format!("https://{rest}")
}

fn port_file() -> std::path::PathBuf {
    crate::store::data_dir().join("instance")
}

/// Hands `arg` to an already running instance. True when one took it (this process should exit).
pub fn forward_to_running(arg: &str) -> bool {
    let Ok(s) = std::fs::read_to_string(port_file()) else { return false };
    let mut parts = s.split_whitespace();
    let (Some(port), pid) = (parts.next().and_then(|p| p.parse::<u16>().ok()), parts.next().and_then(|p| p.parse::<u32>().ok())) else {
        return false;
    };
    // Ignore a port file left by an instance that has exited.
    if pid.is_some_and(|p| !process_alive(p)) {
        return false;
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else { return false };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
    if stream.write_all(format!("STREAMTUBE {}\n", arg.replace('\n', " ")).as_bytes()).is_err() {
        return false;
    }
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).is_ok() && reply.trim() == "ok"
}

/// Whether a process with this id is running.
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else { return false };
    // SAFETY: signal 0 queries a positive PID without sending a signal.
    pid > 0 && (unsafe { libc::kill(pid, 0) } == 0
        || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}

/// Listens for links from later launches; each arrives on the returned channel.
pub fn listen(tx: futures::channel::mpsc::UnboundedSender<String>) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    std::fs::write(port_file(), format!("{port} {}", std::process::id()))?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut line = String::new();
            let mut reader = BufReader::new(&stream);
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            // only our own launches speak this
            let Some(arg) = line.trim_end_matches(['\r', '\n']).strip_prefix("STREAMTUBE ") else { continue };
            let _ = (&stream).write_all(b"ok\n");
            if tx.unbounded_send(arg.to_string()).is_err() {
                break;
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn alive() {
        assert!(super::process_alive(std::process::id()));
        assert!(!super::process_alive(u32::MAX - 1));
    }

    #[test]
    fn scheme_links() {
        assert_eq!(normalize("xdytube://open?url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3Dabc"), "https://www.youtube.com/watch?v=abc");
        assert_eq!(normalize("xdytube:https://youtu.be/abc"), "https://youtu.be/abc");
        assert_eq!(normalize("xdytube://https:/youtu.be/abc"), "https://youtu.be/abc");
        assert_eq!(normalize("xdytube://www.youtube.com/watch?v=abc"), "https://www.youtube.com/watch?v=abc");
        assert_eq!(normalize("https://youtu.be/abc"), "https://youtu.be/abc");
    }
}
