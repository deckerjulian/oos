// Copyright (C) 2026 Jeroen
// SPDX-License-Identifier: GPL-2.0-or-later

//! The Rust files of a crate as the compiler sees them: every module reachable from a target's root file, and
//! whether it is test code (an integration test, or below a `#[cfg(test)]` module).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit::{self, Visit};

/// One parsed source file.
pub struct Source {
    /// path relative to the repository root, with `/`
    pub rel: String,
    /// the whole file is test code
    pub test: bool,
    pub file: syn::File,
}

/// A cfg predicate that cannot hold without `test`: `test`, `all(test, ..)`, `any(test, test)`.
fn requires_test(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(p) => p.is_ident("test"),
        syn::Meta::List(list) => {
            let Ok(items) =
                list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
            else {
                return false;
            };
            if list.path.is_ident("all") {
                items.iter().any(requires_test)
            } else if list.path.is_ident("any") {
                !items.is_empty() && items.iter().all(requires_test)
            } else {
                false
            }
        }
        syn::Meta::NameValue(_) => false,
    }
}

/// `#[cfg(test)]` or a compound predicate that needs it.
pub fn cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("cfg") && a.parse_args::<syn::Meta>().is_ok_and(|m| requires_test(&m)))
}

fn path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find(|a| a.path().is_ident("path")).and_then(|a| match &a.meta {
        syn::Meta::NameValue(nv) => match &nv.value {
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    })
}

/// The `mod x;` declarations of one file, resolved to file paths.
struct Decls<'p> {
    /// the directory of the file itself (for `#[path]`) and the directory its child modules live in
    file_dir: &'p Path,
    mod_dir: &'p Path,
    /// the inline modules around the current point
    inline: Vec<String>,
    test: Vec<bool>,
    out: Vec<(PathBuf, bool)>,
}

impl<'a> Visit<'a> for Decls<'_> {
    fn visit_item_mod(&mut self, m: &'a syn::ItemMod) {
        let test = self.test.last().copied().unwrap_or(false) || cfg_test(&m.attrs);
        let name = m.ident.to_string();
        match &m.content {
            None => {
                let base = if self.inline.is_empty() { self.file_dir } else { self.mod_dir };
                let mut dir =
                    if path_attr(&m.attrs).is_some() { base.to_path_buf() } else { self.mod_dir.to_path_buf() };
                dir.extend(&self.inline);
                let file = match path_attr(&m.attrs) {
                    Some(p) => dir.join(p),
                    None if dir.join(format!("{name}.rs")).exists() => dir.join(format!("{name}.rs")),
                    None => dir.join(&name).join("mod.rs"),
                };
                self.out.push((file, test));
            }
            Some(_) => {
                self.inline.push(name);
                self.test.push(test);
                visit::visit_item_mod(self, m);
                self.test.pop();
                self.inline.pop();
            }
        }
    }
}

/// The directory the child modules of `file` live in: its own directory for a root or `mod.rs`, else one named
/// after it.
fn mod_dir(file: &Path, root: bool) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
    match file.file_stem().and_then(|s| s.to_str()) {
        Some("mod") => dir,
        _ if root => dir,
        Some(stem) => dir.join(stem),
        None => dir,
    }
}

fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .map(|d| {
            d.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "rs")).collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// The target roots of the crate in `dir`: library, binaries, examples (production) and integration tests and
/// benches (test code).
fn roots(dir: &Path) -> Vec<(PathBuf, bool)> {
    let mut out = Vec::new();
    for f in ["src/lib.rs", "src/main.rs"] {
        if dir.join(f).exists() {
            out.push((dir.join(f), false));
        }
    }
    out.extend(rs_files(&dir.join("src/bin")).into_iter().map(|p| (p, false)));
    out.extend(rs_files(&dir.join("examples")).into_iter().map(|p| (p, false)));
    out.extend(rs_files(&dir.join("tests")).into_iter().map(|p| (p, true)));
    out.extend(rs_files(&dir.join("benches")).into_iter().map(|p| (p, true)));
    out
}

/// Every source file of the crate in `repo/krate`. A file reached as production and as test code counts as
/// production: its tests are then found by their own `#[test]` and `#[cfg(test)]`.
pub fn of_crate(repo: &Path, krate: &str) -> Result<Vec<Source>, String> {
    let mut seen: BTreeMap<PathBuf, bool> = BTreeMap::new();
    let mut todo: Vec<(PathBuf, bool, bool)> =
        roots(&repo.join(krate)).into_iter().map(|(p, t)| (p, t, true)).collect();
    let mut parsed: BTreeMap<PathBuf, syn::File> = BTreeMap::new();
    while let Some((path, test, root)) = todo.pop() {
        match seen.get(&path) {
            Some(&old) if old <= test => continue,
            _ => {}
        }
        seen.insert(path.clone(), test);
        if !parsed.contains_key(&path) {
            let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let file = syn::parse_file(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            parsed.insert(path.clone(), file);
        }
        let file_dir = path.parent().unwrap_or(Path::new("."));
        let dir = mod_dir(&path, root);
        let mut d = Decls { file_dir, mod_dir: &dir, inline: Vec::new(), test: vec![test], out: Vec::new() };
        d.visit_file(&parsed[&path]);
        todo.extend(d.out.into_iter().map(|(p, t)| (p, t, false)));
    }
    let mut out = Vec::new();
    for (path, file) in parsed {
        let rel = path
            .strip_prefix(repo)
            .unwrap_or(&path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        out.push(Source { rel, test: seen[&path], file });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modules_are_followed_with_their_test_context() {
        let repo = std::env::temp_dir().join(format!("xtask-sources-{}", std::process::id()));
        let w = |p: &str, text: &str| {
            let p = repo.join(p);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        w("c/src/lib.rs", "mod a;\n#[cfg(test)]\nmod tests;\n");
        w("c/src/a.rs", "mod b;\n#[cfg(test)]\n#[path = \"a_tests.rs\"]\nmod t;\n");
        w("c/src/a/b.rs", "");
        w("c/src/a_tests.rs", "");
        w("c/src/tests.rs", "mod helper;\n");
        w("c/src/tests/helper.rs", "");
        w("c/tests/it.rs", "");
        let mut got: Vec<(String, bool)> = of_crate(&repo, "c").unwrap().into_iter().map(|s| (s.rel, s.test)).collect();
        got.sort();
        fs::remove_dir_all(&repo).unwrap();
        let want = [
            ("c/src/a.rs", false),
            ("c/src/a/b.rs", false),
            ("c/src/a_tests.rs", true),
            ("c/src/lib.rs", false),
            ("c/src/tests.rs", true),
            ("c/src/tests/helper.rs", true),
            ("c/tests/it.rs", true),
        ];
        assert_eq!(got, want.map(|(r, t)| (r.to_string(), t)));
    }
}
