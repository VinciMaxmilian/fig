//! How `figcraft-cli mcp --app` finds the running app: the app writes `mcp-endpoint.json` (its
//! port, a token for this run and its process id) in its config folder, readable by its user only,
//! and removes it when it quits. The token is what keeps other programs, and web pages, from
//! driving the app through that port.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

const FILE: &str = "mcp-endpoint.json";

/// Where a running app can be reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

impl Endpoint {
    /// The address to connect to.
    pub fn addr(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

/// The app's config folder (preferences live here too).
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    #[cfg(not(any(unix, windows)))]
    let base: Option<PathBuf> = None;
    base.map(|b| b.join("FigCraft"))
}

/// The endpoint file inside `dir`.
pub fn file_in(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Keep `endpoint` in `dir` (a temporary file renamed into place, so a reader never sees half of it).
pub fn write(dir: &Path, endpoint: &Endpoint) -> std::io::Result<()> {
    use std::io::Write as _;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("{FILE}.tmp"));
    let text = json!({"port": endpoint.port, "token": endpoint.token, "pid": endpoint.pid}).to_string();
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(text.as_bytes())?;
    drop(file);
    std::fs::rename(&tmp, file_in(dir)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// The endpoint kept in `dir`, or why there is none.
pub fn read(dir: &Path) -> Result<Endpoint, String> {
    let path = file_in(dir);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let port = v.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()).filter(|p| *p != 0);
    let token = v.get("token").and_then(Value::as_str).filter(|t| !t.is_empty());
    match (port, token) {
        (Some(port), Some(token)) => Ok(Endpoint {
            port,
            token: token.to_string(),
            pid: v.get("pid").and_then(Value::as_u64).and_then(|p| u32::try_from(p).ok()).unwrap_or(0),
        }),
        _ => Err(format!("{}: not an endpoint file", path.display())),
    }
}

/// Forget the endpoint kept in `dir` (the app is quitting), if it is still `endpoint`'s.
pub fn remove(dir: &Path, endpoint: &Endpoint) {
    if read(dir).is_ok_and(|kept| kept == *endpoint) {
        let _ = std::fs::remove_file(file_in(dir));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("figcraft-discovery-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn an_endpoint_round_trips_and_only_its_owner_removes_it() {
        let d = dir("round");
        let a = Endpoint { port: 51234, token: "abc".into(), pid: 7 };
        write(&d, &a).unwrap();
        assert_eq!(read(&d).unwrap(), a);
        assert_eq!(a.addr(), "127.0.0.1:51234");
        // Another run took the file over: the first one quitting must leave it.
        let b = Endpoint { port: 51235, token: "def".into(), pid: 8 };
        write(&d, &b).unwrap();
        remove(&d, &a);
        assert_eq!(read(&d).unwrap(), b);
        remove(&d, &b);
        assert!(read(&d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn junk_is_an_error_not_a_panic() {
        let d = dir("junk");
        std::fs::create_dir_all(&d).unwrap();
        for text in ["", "[]", "{}", r#"{"port":0,"token":"x"}"#, r#"{"port":70000,"token":"x"}"#, r#"{"port":5,"token":""}"#, "\u{0}"] {
            std::fs::write(file_in(&d), text).unwrap();
            assert!(read(&d).is_err(), "{text:?}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
