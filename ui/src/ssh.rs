// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! The SSH server of System > Network > Remote services (docs/network.md 7). `/bin/oos-ssh` runs Dropbear and makes
//! the login password and the host keys at the first start; oos-ui reads what it made, keeps the trusted keys file
//! (`authorized_keys`, imported from a USB stick) and asks the script to start or stop the server. All files live in
//! the SSH folder: `/oos/ssh` on the data partition, `/tmp/oos-ssh` without one.

use std::io::Write;
use std::path::{Path, PathBuf};

/// The script that owns the server.
const SCRIPT: &str = "/bin/oos-ssh";
/// One line of state the script leaves: `on: keys only`, `off: needs net.allow`, ...
const STATE: &str = "/tmp/oos-ssh.state";
const LOG: &str = "/tmp/oos-ssh.log";
pub const KEYS_FILE: &str = "authorized_keys";
const PASSWORD_FILE: &str = "password";
const HOST_KEY_PUB: &str = "dropbear_ed25519_host_key.pub";
/// Trusted keys at most (Dropbear reads 300 lines of the file).
pub const KEYS_MAX: usize = 100;
/// A larger file on a stick is not a public key file.
const IMPORT_FILE_MAX: u64 = 256 * 1024;
/// RSA keys below this are refused.
const RSA_BITS_MIN: usize = 2048;

/// Key types Dropbear takes.
const TYPES: [(&str, &str); 7] = [
    ("ssh-ed25519", "ed25519"),
    ("ecdsa-sha2-nistp256", "ecdsa"),
    ("ecdsa-sha2-nistp384", "ecdsa"),
    ("ecdsa-sha2-nistp521", "ecdsa"),
    ("ssh-rsa", "rsa"),
    ("sk-ssh-ed25519@openssh.com", "ed25519-sk"),
    ("sk-ecdsa-sha2-nistp256@openssh.com", "ecdsa-sk"),
];

/// The SSH folder: on the data partition when it is there (it has `/oos/setups`, like `sysconf::default_path`).
pub fn dir() -> PathBuf {
    if Path::new("/oos/setups").is_dir() {
        "/oos/ssh".into()
    } else {
        "/tmp/oos-ssh".into()
    }
}

/// A public key of an `authorized_keys` line.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub kind: String,
    pub blob: Vec<u8>,
    pub comment: String,
    /// the line starts with options (`from=...`, `command=...`): kept as written, not imported from a stick
    pub options: bool,
}

impl Key {
    /// As `ssh-keygen -l` prints it: `SHA256:` and the unpadded base64 of the SHA-256 of the key.
    pub fn fingerprint(&self) -> String {
        let h = oos_hal::update::hash::sha256(&self.blob);
        format!("SHA256:{}", crate::http::base64(&h).trim_end_matches('='))
    }

    pub fn short_kind(&self) -> &'static str {
        TYPES.iter().find(|t| t.0 == self.kind).map_or("key", |t| t.1)
    }

    /// One line for a list or a picker: type, comment, the start of the fingerprint.
    pub fn label(&self) -> String {
        let fp = self.fingerprint();
        let comment = if self.comment.is_empty() { "(no comment)".to_string() } else { self.comment.chars().take(28).collect() };
        format!("{} {} {}\u{2026}", self.short_kind(), comment, &fp[..fp.len().min(19)])
    }

    /// The line it is saved as.
    fn line(&self) -> String {
        let b = crate::http::base64(&self.blob);
        if self.comment.is_empty() {
            format!("{} {}", self.kind, b)
        } else {
            format!("{} {} {}", self.kind, b, self.comment)
        }
    }
}

fn b64_val(c: u8) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    } as u32)
}

/// Standard base64 (RFC 4648), padding optional; None for anything else.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let t = s.trim_end_matches('=');
    if s.len() - t.len() > 2 || t.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(t.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for &c in t.as_bytes() {
        acc = (acc << 6) | b64_val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // the bits left over must be zero (one encoding per key)
    (acc == 0).then_some(out)
}

/// The next SSH wire string (u32 big-endian length, bytes) of `b` from `*at`.
fn wire_string<'a>(b: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let n = u32::from_be_bytes(b.get(*at..*at + 4)?.try_into().ok()?) as usize;
    let s = b.get(*at + 4..(*at + 4).checked_add(n)?)?;
    *at += 4 + n;
    Some(s)
}

/// The key of one `authorized_keys` line: `[options] type base64 [comment]`. The blob must name the same type, an RSA
/// key must have at least 2048 bits.
pub fn parse_line(line: &str) -> Result<Key, String> {
    let l = line.trim();
    let is_type = |w: &str| TYPES.iter().any(|t| t.0 == w);
    let first = l.split_whitespace().next().unwrap_or("");
    let (options, rest) = if is_type(first) {
        (false, l)
    } else {
        // options end at the first blank outside double quotes
        let (mut quoted, mut prev) = (false, ' ');
        let end = l
            .char_indices()
            .find(|&(_, c)| {
                if c == '"' && prev != '\\' {
                    quoted = !quoted;
                }
                prev = c;
                c.is_whitespace() && !quoted
            })
            .map(|(i, _)| i)
            .ok_or("not a public key")?;
        (true, l[end..].trim_start())
    };
    let mut w = rest.splitn(3, char::is_whitespace);
    let kind = w.next().unwrap_or("");
    if !is_type(kind) {
        return Err(if kind.contains("PRIVATE") || l.contains("PRIVATE KEY") { "a private key, not a public one".into() } else { "not a public key".into() });
    }
    let blob = w.next().and_then(base64_decode).ok_or("key data is not base64")?;
    let mut at = 0;
    if wire_string(&blob, &mut at) != Some(kind.as_bytes()) {
        return Err("key data does not match its type".into());
    }
    if kind == "ssh-rsa" {
        let _e = wire_string(&blob, &mut at).ok_or("RSA key cut short")?;
        let n = wire_string(&blob, &mut at).ok_or("RSA key cut short")?;
        let n = &n[n.iter().position(|&b| b != 0).unwrap_or(n.len())..];
        let bits = n.len() * 8 - n.first().map_or(8, |b| b.leading_zeros() as usize);
        if bits < RSA_BITS_MIN {
            return Err(format!("RSA key of {} bits, at least {} needed", bits, RSA_BITS_MIN));
        }
    }
    let comment = w.next().unwrap_or("").trim().to_string();
    Ok(Key { kind: kind.into(), blob, comment, options })
}

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Import {
    pub added: usize,
    /// already trusted
    pub known: usize,
    /// lines that are not a key, with the reason
    pub bad: Vec<String>,
    /// left out: the list is full
    pub full: usize,
}

impl Import {
    /// For a toast: `2 keys added, 1 already trusted, 1 not a key (a private key, not a public one)`.
    pub fn summary(&self) -> String {
        let n = |k: usize, one: &str| format!("{} {}{}", k, one, if k == 1 { "" } else { "s" });
        let mut parts = vec![format!("{} added", n(self.added, "key"))];
        if self.known > 0 {
            parts.push(format!("{} already trusted", self.known));
        }
        if self.full > 0 {
            parts.push(format!("{} left out (at most {})", self.full, KEYS_MAX));
        }
        if let Some(first) = self.bad.first() {
            parts.push(format!("{} not taken ({})", self.bad.len(), first));
        }
        parts.join(", ")
    }
}

/// The trusted keys file. Lines this code does not read as a key (comments, blank lines, unknown types) stay as they
/// are.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keys {
    lines: Vec<String>,
}

impl Keys {
    pub fn parse(text: &str) -> Keys {
        Keys { lines: text.lines().map(str::to_string).collect() }
    }

    pub fn text(&self) -> String {
        self.lines.iter().map(|l| format!("{}\n", l)).collect()
    }

    /// The keys, in the order of the file.
    pub fn keys(&self) -> Vec<Key> {
        self.lines.iter().filter(|l| !l.trim().starts_with('#')).filter_map(|l| parse_line(l).ok()).collect()
    }

    /// Adds the keys of `text` (the files of a stick): plain keys only, each once.
    pub fn import(&mut self, text: &str) -> Import {
        let mut r = Import::default();
        let mut have = self.keys();
        for l in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            match parse_line(l) {
                Ok(k) if k.options => r.bad.push("options are not imported".into()),
                Ok(k) if have.iter().any(|h| h.blob == k.blob) => r.known += 1,
                Ok(_) if have.len() >= KEYS_MAX => r.full += 1,
                Ok(k) => {
                    self.lines.push(k.line());
                    have.push(k);
                    r.added += 1;
                }
                Err(e) => r.bad.push(e),
            }
        }
        r
    }

    /// Removes the `n`th key of `keys()`.
    pub fn remove(&mut self, n: usize) -> Option<Key> {
        let mut seen = 0;
        for i in 0..self.lines.len() {
            let l = &self.lines[i];
            if l.trim().starts_with('#') {
                continue;
            }
            if let Ok(k) = parse_line(l) {
                if seen == n {
                    self.lines.remove(i);
                    return Some(k);
                }
                seen += 1;
            }
        }
        None
    }

    /// From the SSH folder; no file is no keys.
    pub fn load(dir: &Path) -> std::io::Result<Keys> {
        match std::fs::read(dir.join(KEYS_FILE)) {
            Ok(b) => Ok(Keys::parse(&String::from_utf8_lossy(&b))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Keys::default()),
            Err(e) => Err(e),
        }
    }

    /// Replaces the file in one step, readable by root only, and syncs it (the scope is switched off at the mains).
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        let tmp = dir.join(format!("{}.new", KEYS_FILE));
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(self.text().as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, dir.join(KEYS_FILE))?;
        std::fs::File::open(dir)?.sync_all()
    }
}

/// Password login may only go off while a trusted key is there: else nobody can log in any more.
pub fn password_off_allowed(keys: usize) -> Result<(), &'static str> {
    if keys == 0 {
        Err("Import a trusted key first: password login off without one locks root out")
    } else {
        Ok(())
    }
}

/// The last key may only go while password login is on.
pub fn remove_allowed(keys: usize, password_on: bool) -> Result<(), &'static str> {
    if keys <= 1 && !password_on {
        Err("Password login is off: removing the last key would lock root out")
    } else {
        Ok(())
    }
}

/// Public key files on a stick mounted at `root`: `*.pub` and `authorized_keys`, in its top folder and in `OOS/`.
pub fn stick_files(root: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for d in [root.to_path_buf(), root.join("OOS")] {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let ok_name = name == KEYS_FILE || (name.ends_with(".pub") && !name.starts_with('.'));
            if ok_name && e.metadata().is_ok_and(|m| m.is_file() && m.len() <= IMPORT_FILE_MAX) {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

/// The text of all key files on the stick, or why there is none.
pub fn stick_text(root: &Path) -> Result<String, String> {
    let files = stick_files(root);
    if files.is_empty() {
        return Err("No *.pub or authorized_keys on the stick (top folder or OOS)".into());
    }
    let mut s = String::new();
    for f in files {
        let b = std::fs::read(&f).map_err(|e| format!("{}: {}", f.display(), e))?;
        s.push_str(&String::from_utf8_lossy(&b));
        s.push('\n');
    }
    Ok(s)
}

/// The login password the script made, if it did.
pub fn password(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(PASSWORD_FILE)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The fingerprint of the server's ed25519 host key (what a client shows at the first connection).
pub fn host_fingerprint(dir: &Path) -> Option<String> {
    let t = std::fs::read_to_string(dir.join(HOST_KEY_PUB)).ok()?;
    parse_line(&t).ok().map(|k| k.fingerprint())
}

/// The script's state line; `not started` before it ran.
pub fn state() -> String {
    std::fs::read_to_string(STATE).map(|s| s.trim().to_string()).unwrap_or_else(|_| "not started".into())
}

/// Starts the server again with the current settings (`on`) or stops it with its sessions. The script runs in its
/// own thread: the first start makes host keys, and the frame loop does not wait.
pub fn apply(on: bool) {
    let arg = if on { "restart" } else { "stop" };
    std::thread::spawn(move || {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(LOG);
        let mut c = std::process::Command::new(SCRIPT);
        c.arg(arg).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null());
        if let Ok(f) = log {
            c.stderr(f);
        }
        match c.status() {
            Ok(s) if s.success() => {}
            Ok(s) => eprintln!("oos-ui: {} {}: {}", SCRIPT, arg, s),
            Err(e) => eprintln!("oos-ui: {} {}: {}", SCRIPT, arg, e),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // ssh-keygen -t ed25519 -C tester@host (public part only)
    const ED: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHpflgL+YU3dK6EtW+scyOzamlWiPXJSwQWA1mfwdC9N tester@host";

    fn wire(parts: &[&[u8]]) -> Vec<u8> {
        parts.iter().flat_map(|p| (p.len() as u32).to_be_bytes().into_iter().chain(p.iter().copied())).collect()
    }

    fn rsa_line(bits: usize) -> String {
        let mut n = vec![0u8; 1 + bits / 8];
        n[1] = 0x80;
        format!("ssh-rsa {} r@h", crate::http::base64(&wire(&[b"ssh-rsa", &[1, 0, 1], &n])))
    }

    #[test]
    fn base64_round_trip_and_rejects() {
        for v in [&b""[..], b"f", b"fo", b"foo", b"foobar", &[0u8, 255, 128, 7, 9]] {
            assert_eq!(base64_decode(&crate::http::base64(v)).as_deref(), Some(v));
        }
        assert_eq!(base64_decode("Zm9vYg").as_deref(), Some(&b"foob"[..]));
        for bad in ["Zm9v!", "Z", "Zm9vY===", "Zh=="] {
            assert_eq!(base64_decode(bad), None, "{}", bad);
        }
    }

    #[test]
    fn a_key_line_and_its_fingerprint() {
        let k = parse_line(ED).unwrap();
        assert_eq!((k.kind.as_str(), k.comment.as_str(), k.options), ("ssh-ed25519", "tester@host", false));
        assert_eq!(k.blob.len(), 51);
        // ssh-keygen -lf of the same line
        assert_eq!(k.fingerprint(), "SHA256:iVbN5nuqu0ix8KlBKoSubOtEb6Bote9ERw0dvOjBMo8");
        assert!(k.label().starts_with("ed25519 tester@host SHA256:"));
        assert_eq!(parse_line(&k.line()).unwrap(), k);
        let k = parse_line("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHpflgL+YU3dK6EtW+scyOzamlWiPXJSwQWA1mfwdC9N").unwrap();
        assert!(k.comment.is_empty());
    }

    #[test]
    fn options_with_quoted_blanks() {
        let l = format!("from=\"192.0.2.0/24\",command=\"echo a b\" {}", ED);
        let k = parse_line(&l).unwrap();
        assert!(k.options && k.comment == "tester@host");
        assert!(parse_line("from=\"x\"").is_err());
    }

    #[test]
    fn what_is_not_a_key() {
        let cases = [
            ("", "not a public key"),
            ("hello world", "not a public key"),
            ("-----BEGIN OPENSSH PRIVATE KEY-----", "a private key, not a public one"),
            ("ssh-ed25519 !!!! x", "key data is not base64"),
            ("ssh-dss AAAAB3NzaC1kc3M= x", "not a public key"),
        ];
        for (l, want) in cases {
            assert_eq!(parse_line(l).unwrap_err(), want, "{}", l);
        }
        // an ed25519 blob under another type name
        let swapped = ED.replacen("ssh-ed25519", "ecdsa-sha2-nistp256", 1);
        assert_eq!(parse_line(&swapped).unwrap_err(), "key data does not match its type");
        assert!(parse_line(&rsa_line(2048)).is_ok());
        assert_eq!(parse_line(&rsa_line(1024)).unwrap_err(), "RSA key of 1024 bits, at least 2048 needed");
    }

    #[test]
    fn import_adds_each_key_once_and_keeps_other_lines() {
        let mut ks = Keys::parse("# mine\nfrom=\"10.0.0.1\" ssh-rsa broken\n");
        let r = ks.import(&format!("{}\n\n{}\n{}\n-----BEGIN OPENSSH PRIVATE KEY-----\nfrom=\"x\" {}\n", ED, ED, rsa_line(4096), ED));
        assert_eq!((r.added, r.known, r.full), (2, 1, 0));
        assert_eq!(r.bad, vec!["a private key, not a public one".to_string(), "options are not imported".to_string()]);
        assert_eq!(r.summary(), "2 keys added, 1 already trusted, 2 not taken (a private key, not a public one)");
        assert_eq!(ks.keys().len(), 2);
        assert!(ks.text().starts_with("# mine\nfrom=\"10.0.0.1\" ssh-rsa broken\nssh-ed25519 "));
        assert_eq!(ks.import(ED).summary(), "0 keys added, 1 already trusted");
    }

    #[test]
    fn import_stops_at_the_limit() {
        let mut ks = Keys::default();
        let many: String = (0..KEYS_MAX + 3).map(|i| format!("{}\n", rsa_line(2048 + 8 * i))).collect();
        let r = ks.import(&many);
        assert_eq!((r.added, r.full), (KEYS_MAX, 3));
    }

    #[test]
    fn remove_takes_the_nth_key_only() {
        let mut ks = Keys::parse(&format!("# c\n{}\nnot a key\n{}\n", rsa_line(2048), ED));
        assert_eq!(ks.remove(1).unwrap().kind, "ssh-ed25519");
        assert_eq!(ks.text(), format!("# c\n{}\nnot a key\n", rsa_line(2048)));
        assert!(ks.remove(1).is_none());
        assert_eq!(ks.remove(0).unwrap().kind, "ssh-rsa");
        assert_eq!(ks.text(), "# c\nnot a key\n");
    }

    #[test]
    fn lockout_guard() {
        assert!(password_off_allowed(0).is_err());
        assert!(password_off_allowed(1).is_ok());
        assert!(remove_allowed(1, false).is_err());
        assert!(remove_allowed(0, false).is_err());
        assert!(remove_allowed(2, false).is_ok());
        assert!(remove_allowed(1, true).is_ok());
    }

    #[test]
    fn saved_loaded_and_found_on_a_stick() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("oos-ssh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let ssh = d.join("ssh");
        assert_eq!(Keys::load(&ssh).unwrap(), Keys::default());
        let mut ks = Keys::default();
        ks.import(ED);
        ks.save(&ssh).unwrap();
        assert_eq!(Keys::load(&ssh).unwrap(), ks);
        assert_eq!(std::fs::metadata(ssh.join(KEYS_FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&ssh).unwrap().permissions().mode() & 0o777, 0o700);
        assert!(!ssh.join("authorized_keys.new").exists());
        // a stick: keys in the top folder and in OOS/, a private key and other files left alone
        let stick = d.join("usb");
        std::fs::create_dir_all(stick.join("OOS")).unwrap();
        std::fs::write(stick.join("id_ed25519.pub"), format!("{}\n", ED)).unwrap();
        std::fs::write(stick.join("id_ed25519"), "-----BEGIN OPENSSH PRIVATE KEY-----\n").unwrap();
        std::fs::write(stick.join("OOS").join("authorized_keys"), rsa_line(3072)).unwrap();
        std::fs::write(stick.join("notes.txt"), "x").unwrap();
        let names: Vec<String> = stick_files(&stick).iter().map(|p| p.strip_prefix(&stick).unwrap().display().to_string()).collect();
        assert_eq!(names, ["OOS/authorized_keys", "id_ed25519.pub"]);
        let r = ks.import(&stick_text(&stick).unwrap());
        assert_eq!((r.added, r.known), (1, 1));
        assert!(stick_text(&d.join("empty")).is_err());
        // what the script made
        std::fs::write(ssh.join(PASSWORD_FILE), "abcd-efgh-jkmn-pqrs\n").unwrap();
        assert_eq!(password(&ssh).as_deref(), Some("abcd-efgh-jkmn-pqrs"));
        std::fs::write(ssh.join(HOST_KEY_PUB), format!("{}\n", ED)).unwrap();
        assert_eq!(host_fingerprint(&ssh), Some(parse_line(ED).unwrap().fingerprint()));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
