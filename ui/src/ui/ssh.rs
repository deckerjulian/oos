// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! System > Network > Remote services > SSH settings (docs/network.md 7): the server's state, password login, the
//! generated password on demand, the host key, and the trusted keys (imported from a USB stick, listed, removed).
//! The switch for SSH itself stands with the other services (ui/remote.rs). `crate::ssh` has the files and the rules.

use super::*;
use crate::ssh::{self, Keys};

/// Keys listed in the dialog; the rest is counted.
const LIST_SHOWN: usize = 10;

pub(super) struct SshUi {
    /// the SSH folder; none in host tests and before `set_ssh_dir`
    pub dir: Option<std::path::PathBuf>,
    /// where the USB stick is mounted
    pub stick: std::path::PathBuf,
    pub keys: Vec<ssh::Key>,
    pub state: String,
    pub host_fp: Option<String>,
    /// when the files were last read (`Ui::now`); None: read at the next poll
    read_at: Option<f64>,
}

impl Default for SshUi {
    fn default() -> Self {
        SshUi { dir: None, stick: "/mnt/usb".into(), keys: Vec::new(), state: String::new(), host_fp: None, read_at: None }
    }
}

impl Ui {
    /// The SSH folder (main.rs, on the unit).
    pub fn set_ssh_dir(&mut self, d: std::path::PathBuf) {
        self.ssh.dir = Some(d);
        self.ssh.read_at = None;
    }

    /// Reads the keys, the state and the host key again once a second while one of the two pages is open: the script
    /// changes the state, `ssh-copy-id` the keys.
    pub(super) fn ssh_poll(&mut self) {
        let open = self.menu_open && matches!(self.menu_root(), MenuId::Ssh | MenuId::Remote);
        if !open || self.ssh.read_at.is_some_and(|t| self.now - t < 1.0) {
            return;
        }
        self.ssh_read();
    }

    fn ssh_read(&mut self) {
        self.ssh.read_at = Some(self.now);
        let Some(d) = self.ssh.dir.clone() else { return };
        match Keys::load(&d) {
            Ok(k) => self.ssh.keys = k.keys(),
            Err(e) => eprintln!("oos-ui: {}: {}", d.join(ssh::KEYS_FILE).display(), e),
        }
        self.ssh.state = ssh::state();
        self.ssh.host_fp = ssh::host_fingerprint(&d);
    }

    pub(super) fn ssh_items(&self) -> Vec<Item> {
        vec![Item::SshState, Item::SshPassword, Item::SshShowPass, Item::SshHostKey, Item::SshKeys, Item::SshKeyList, Item::SshImport, Item::SshRemove]
    }

    pub(super) fn ssh_kind(it: Item) -> Option<Kind> {
        match it {
            Item::SshPassword => Some(Kind::Choice),
            Item::SshShowPass | Item::SshKeyList | Item::SshImport | Item::SshRemove => Some(Kind::Action),
            _ => None,
        }
    }

    pub(super) fn ssh_text(it: Item) -> bool {
        matches!(it, Item::SshState | Item::SshHostKey | Item::SshKeys)
    }

    pub(super) fn ssh_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Ssh) => "SSH settings",
            Item::SshState => "State",
            Item::SshPassword => "Password login",
            Item::SshShowPass => "Show password",
            Item::SshHostKey => "Host key",
            Item::SshKeys => "Trusted keys",
            Item::SshKeyList => "List keys",
            Item::SshImport => "Import keys from USB",
            Item::SshRemove => "Remove a key",
            _ => return None,
        })
    }

    pub(super) fn ssh_value(&self, it: Item) -> Option<String> {
        let s = &self.ssh;
        Some(match it {
            Item::SshState if !self.sys.remote_ssh => "off".into(),
            Item::SshState if s.state.is_empty() => "not started".into(),
            Item::SshState => s.state.clone(),
            Item::SshPassword => if self.sys.ssh_password { "On" } else { "Off" }.into(),
            Item::SshHostKey => s.host_fp.as_ref().map_or("not made yet".into(), |f| format!("{}\u{2026}", &f[..f.len().min(19)])),
            Item::SshKeys if s.keys.is_empty() => "none".into(),
            Item::SshKeys => s.keys.len().to_string(),
            Item::SshRemoveKey => String::new(),
            _ => return None,
        })
    }

    pub(super) fn ssh_options(&self, it: Item) -> Option<Vec<String>> {
        match it {
            Item::SshPassword => Some(vec!["Off".into(), "On".into()]),
            Item::SshRemoveKey => Some(self.ssh.keys.iter().map(|k| k.label()).collect()),
            _ => None,
        }
    }

    /// Password login on or off; off only while a trusted key is there.
    pub(super) fn ssh_adjust(&mut self, it: Item, _d: i32) -> bool {
        if it != Item::SshPassword {
            return false;
        }
        if self.sys.ssh_password {
            if let Err(m) = ssh::password_off_allowed(self.ssh.keys.len()) {
                self.toast(m);
                return true;
            }
        }
        self.sys.ssh_password = !self.sys.ssh_password;
        self.sys_save();
        self.toast(if self.sys.ssh_password { "Password login on" } else { "Password login off: trusted keys only" });
        true
    }

    pub(super) fn ssh_activate(&mut self, it: Item) -> bool {
        match it {
            Item::SshShowPass => self.dialog = Some(self.ssh_password_lines()),
            Item::SshKeyList => self.dialog = Some(self.ssh_key_lines()),
            Item::SshImport => self.ssh_import(),
            Item::SshRemove => {
                if self.ssh.keys.is_empty() {
                    self.toast("No trusted keys");
                } else if let Err(m) = ssh::remove_allowed(self.ssh.keys.len(), self.sys.ssh_password) {
                    self.toast(m);
                } else {
                    self.open_picker(Item::SshRemoveKey);
                }
            }
            _ => return false,
        }
        true
    }

    fn ssh_login(&self) -> String {
        if self.net.is_empty() {
            "ssh root@<address of the scope>".into()
        } else {
            format!("ssh root@{}", self.net)
        }
    }

    /// The password, only when asked for (the web remote mirrors the screen while this dialog is open).
    fn ssh_password_lines(&self) -> Vec<String> {
        let mut d = vec!["SSH login".to_string(), self.ssh_login()];
        match self.ssh.dir.as_deref().and_then(ssh::password) {
            Some(p) => d.push(format!("Password  {}", p)),
            None => d.push("No password yet: SSH has not started".into()),
        }
        if !self.sys.ssh_password {
            d.push("Password login is off: trusted keys only".into());
        }
        if let Some(f) = &self.ssh.host_fp {
            d.push(format!("Host key  {}", f));
        }
        d
    }

    fn ssh_key_lines(&self) -> Vec<String> {
        let mut d = vec!["Trusted keys".to_string()];
        if self.ssh.keys.is_empty() {
            d.push("None. Import *.pub files from a USB stick,".into());
            d.push("or use ssh-copy-id with the password".into());
        }
        d.extend(self.ssh.keys.iter().take(LIST_SHOWN).map(|k| format!("{}  {}", k.comment, k.fingerprint())));
        if self.ssh.keys.len() > LIST_SHOWN {
            d.push(format!("and {} more", self.ssh.keys.len() - LIST_SHOWN));
        }
        d
    }

    fn ssh_import(&mut self) {
        let Some(dir) = self.ssh.dir.clone() else { return };
        if !self.usb {
            return self.toast("No USB stick in the front port");
        }
        let text = match ssh::stick_text(&self.ssh.stick) {
            Ok(t) => t,
            Err(e) => return self.toast(&e),
        };
        let mut keys = match Keys::load(&dir) {
            Ok(k) => k,
            Err(e) => return self.toast(&format!("Keys not read: {}", e)),
        };
        let r = keys.import(&text);
        if r.added > 0 {
            if let Err(e) = keys.save(&dir) {
                return self.toast(&format!("Not saved: {}", e));
            }
        }
        self.ssh_read();
        self.toast(&r.summary());
    }

    /// A line of the remove picker.
    pub(super) fn ssh_remove_pick(&mut self, k: usize) {
        let Some(dir) = self.ssh.dir.clone() else { return };
        let mut keys = match Keys::load(&dir) {
            Ok(k) => k,
            Err(e) => return self.toast(&format!("Keys not read: {}", e)),
        };
        // the file may have changed since the picker opened (ssh-copy-id): check against what is there now
        let now = keys.keys();
        if now.get(k).map(|x| &x.blob) != self.ssh.keys.get(k).map(|x| &x.blob) {
            self.ssh_read();
            return self.toast("The keys changed, pick again");
        }
        if let Err(m) = ssh::remove_allowed(now.len(), self.sys.ssh_password) {
            return self.toast(m);
        }
        let Some(gone) = keys.remove(k) else { return };
        if let Err(e) = keys.save(&dir) {
            return self.toast(&format!("Not saved: {}", e));
        }
        self.ssh_read();
        self.toast(&format!("Removed {}", if gone.comment.is_empty() { gone.fingerprint() } else { gone.comment }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHpflgL+YU3dK6EtW+scyOzamlWiPXJSwQWA1mfwdC9N tester@host";

    fn with_dir(name: &str) -> (Ui, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("oos-ui-ssh-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("usb")).unwrap();
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.set_ssh_dir(d.join("ssh"));
        u.ssh.stick = d.join("usb");
        u.sys.remote_ssh = true;
        u.open_menu(MenuId::Ssh);
        u.ssh_read();
        (u, d)
    }

    #[test]
    fn password_login_stays_on_without_a_key() {
        let (mut u, d) = with_dir("guard");
        assert_eq!(u.value(Item::SshKeys), "none");
        u.select_option(Item::SshPassword, "Off");
        assert!(u.sys.ssh_password, "no key: password login must stay on");
        assert!(u.toast.as_ref().unwrap().0.contains("locks root out"));
        // a key from the stick, then it may go off
        u.usb = true;
        std::fs::write(d.join("usb").join("id_ed25519.pub"), ED).unwrap();
        u.activate(Item::SshImport);
        assert_eq!(u.toast.as_ref().unwrap().0, "1 key added");
        assert_eq!(u.value(Item::SshKeys), "1");
        u.select_option(Item::SshPassword, "Off");
        assert!(!u.sys.ssh_password);
        // the last key cannot go while password login is off
        u.activate(Item::SshRemove);
        assert!(u.pick.is_none() && u.toast.as_ref().unwrap().0.contains("lock root out"));
        u.select_option(Item::SshPassword, "On");
        u.activate(Item::SshRemove);
        assert_eq!(u.pick.map(|p| p.it), Some(Item::SshRemoveKey));
        assert!(u.options(Item::SshRemoveKey)[0].starts_with("ed25519 tester@host SHA256:"));
        u.activate(Item::PickOpt(0));
        assert_eq!(u.toast.as_ref().unwrap().0, "Removed tester@host");
        assert_eq!(u.value(Item::SshKeys), "none");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn import_needs_a_stick_and_a_key_file() {
        let (mut u, d) = with_dir("import");
        u.activate(Item::SshImport);
        assert_eq!(u.toast.as_ref().unwrap().0, "No USB stick in the front port");
        u.usb = true;
        u.activate(Item::SshImport);
        assert!(u.toast.as_ref().unwrap().0.starts_with("No *.pub"));
        std::fs::write(d.join("usb").join("id_ed25519"), "-----BEGIN OPENSSH PRIVATE KEY-----\n").unwrap();
        std::fs::write(d.join("usb").join("bad.pub"), "-----BEGIN OPENSSH PRIVATE KEY-----\n").unwrap();
        u.activate(Item::SshImport);
        assert_eq!(u.toast.as_ref().unwrap().0, "0 keys added, 1 not taken (a private key, not a public one)");
        assert!(!d.join("ssh").join("authorized_keys").exists(), "nothing written when nothing was added");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_password_only_on_demand() {
        let (mut u, d) = with_dir("show");
        std::fs::create_dir_all(d.join("ssh")).unwrap();
        std::fs::write(d.join("ssh").join("password"), "abcd-efgh-jkmn-pqrs\n").unwrap();
        u.net = "192.0.2.10".into();
        let d_items = u.dlg_layout().unwrap();
        for r in &d_items.rows {
            assert!(!u.value(r.it).contains("abcd"), "{:?} shows the password", r.it);
        }
        u.activate(Item::SshShowPass);
        let lines = u.dialog.clone().unwrap();
        assert_eq!(lines[1], "ssh root@192.0.2.10");
        assert_eq!(lines[2], "Password  abcd-efgh-jkmn-pqrs");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_page_has_two_columns_of_four() {
        let (u, d) = with_dir("layout");
        let dl = u.dlg_layout().unwrap();
        let y = |it: Item| dl.rows.iter().find(|r| r.it == it).unwrap().ctrl.y;
        assert_eq!(y(Item::SshState), y(Item::SshKeys));
        assert_eq!(y(Item::SshHostKey), y(Item::SshRemove));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
