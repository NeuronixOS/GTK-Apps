//! Git-status colors for folder names printed by interactive `ls`.
//!
//! The live path is a bash script ([`git_ls_lib.bash`](git_ls_lib.bash)) so
//! `ls` does not start this GTK binary. The classifier here is the same
//! mapping gtk-files uses, and the unit tests lock both sides to it.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const LIB_SCRIPT: &str = include_str!("git_ls_lib.bash");
const HOOK_TEMPLATE: &str = include_str!("git_ls_hook.sh");

/// How to spawn the user's interactive shell with the `ls` wrapper installed.
pub struct ShellLaunch {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Name-label color: blue / orange / red. Severity increases with the variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GitDecor {
    /// Untracked or newly added (`??`, `A`).
    Untracked,
    /// Tracked dirty (modified, deleted, renamed, …).
    Modified,
    /// Unmerged / both-added / both-deleted.
    Conflict,
}

impl GitDecor {
    fn severity(self) -> u8 {
        match self {
            Self::Untracked => 1,
            Self::Modified => 2,
            Self::Conflict => 3,
        }
    }

    fn merge(self, other: Self) -> Self {
        if other.severity() > self.severity() {
            other
        } else {
            self
        }
    }

    fn merge_opt(a: Option<Self>, b: Option<Self>) -> Option<Self> {
        match (a, b) {
            (None, x) | (x, None) => x,
            (Some(x), Some(y)) => Some(x.merge(y)),
        }
    }

    #[cfg(test)]
    fn name(self) -> &'static str {
        match self {
            Self::Untracked => "untracked",
            Self::Modified => "modified",
            Self::Conflict => "conflict",
        }
    }
}

/// Argv and env for the interactive shell in `SHELL`, or a plain `shell -i`
/// when the shell is not bash/zsh or the cache dir cannot be written.
pub fn interactive_shell() -> ShellLaunch {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let shell = PathBuf::from(shell);
    match interactive_shell_for(&shell) {
        Ok(launch) => launch,
        Err(err) => {
            eprintln!("gtk-term: git ls colors unavailable: {err}");
            plain_shell(&shell)
        }
    }
}

pub fn interactive_shell_for(shell: &Path) -> io::Result<ShellLaunch> {
    let Some(kind) = shell_kind(shell) else {
        return Ok(plain_shell(shell));
    };
    let Some(cache) = dirs::cache_dir().map(|p| p.join("gtk-term")) else {
        return Ok(plain_shell(shell));
    };
    install_into(&cache, shell, kind)
}

fn plain_shell(shell: &Path) -> ShellLaunch {
    ShellLaunch {
        argv: vec![shell.to_string_lossy().into_owned(), "-i".into()],
        env: Vec::new(),
    }
}

fn shell_kind(shell: &Path) -> Option<&'static str> {
    match shell.file_name().and_then(|s| s.to_str()) {
        Some("bash") => Some("bash"),
        Some("zsh") => Some("zsh"),
        _ => None,
    }
}

fn install_into(cache: &Path, shell: &Path, kind: &str) -> io::Result<ShellLaunch> {
    fs::create_dir_all(cache)?;
    let lib = cache.join("git-ls-lib.bash");
    fs::write(&lib, LIB_SCRIPT)?;
    let hook = cache.join("git-ls-hook.sh");
    let hook_body = HOOK_TEMPLATE.replace("__GTK_TERM_GIT_LS_LIB__", &shell_quote(&lib));
    fs::write(&hook, hook_body)?;

    match kind {
        "bash" => {
            let rc = cache.join("git-ls.bashrc");
            fs::write(&rc, bashrc(&hook))?;
            // Long options must precede -i. `bash -i --rcfile` treats
            // `--rcfile` as an illegal short option.
            Ok(ShellLaunch {
                argv: vec![
                    shell.to_string_lossy().into_owned(),
                    "--rcfile".into(),
                    rc.to_string_lossy().into_owned(),
                    "-i".into(),
                ],
                env: Vec::new(),
            })
        }
        "zsh" => {
            let zdot = cache.join("zsh");
            fs::create_dir_all(&zdot)?;
            fs::write(zdot.join(".zshenv"), ZSHENV)?;
            fs::write(zdot.join(".zshrc"), zshrc(&hook))?;
            let user_zdot = user_zdotdir(&zdot);
            Ok(ShellLaunch {
                argv: vec![shell.to_string_lossy().into_owned(), "-i".into()],
                env: vec![
                    ("ZDOTDIR".into(), zdot.to_string_lossy().into_owned()),
                    ("GTK_TERM_USER_ZDOTDIR".into(), user_zdot),
                ],
            })
        }
        _ => Ok(plain_shell(shell)),
    }
}

fn user_zdotdir(our_zdot: &Path) -> String {
    let ours = our_zdot.to_string_lossy();
    let from_env = std::env::var("GTK_TERM_USER_ZDOTDIR")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("ZDOTDIR").ok().filter(|s| !s.is_empty()));
    match from_env {
        Some(path) if path != ours => path,
        _ => std::env::var("HOME").unwrap_or_else(|_| "/".into()),
    }
}

fn bashrc(hook: &Path) -> String {
    format!(
        r#"# gtk-term git-aware ls. --rcfile replaces the normal bashrc, so load it first.
if [ -f /etc/bash.bashrc ]; then
  . /etc/bash.bashrc
fi
if [ -f "${{HOME}}/.bashrc" ]; then
  . "${{HOME}}/.bashrc"
fi
. {hook}
"#,
        hook = shell_quote(hook)
    )
}

const ZSHENV: &str = r#"# gtk-term: chain the user's .zshenv from the real ZDOTDIR.
if [ -n "${GTK_TERM_USER_ZDOTDIR:-}" ] && [ -f "${GTK_TERM_USER_ZDOTDIR}/.zshenv" ]; then
  . "${GTK_TERM_USER_ZDOTDIR}/.zshenv"
fi
"#;

fn zshrc(hook: &Path) -> String {
    format!(
        r#"# gtk-term: user's .zshrc, then git-aware ls.
if [ -n "${{GTK_TERM_ZSH_RC_DONE:-}}" ]; then
  return 0
fi
export GTK_TERM_ZSH_RC_DONE=1
if [ -n "${{GTK_TERM_USER_ZDOTDIR:-}}" ] && [ -f "${{GTK_TERM_USER_ZDOTDIR}}/.zshrc" ]; then
  . "${{GTK_TERM_USER_ZDOTDIR}}/.zshrc"
fi
. {hook}
"#,
        hook = shell_quote(hook)
    )
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

pub fn classify_xy(xy: &str) -> Option<GitDecor> {
    let bytes = xy.as_bytes();
    if bytes.len() != 2 {
        return None;
    }
    let a = bytes[0] as char;
    let b = bytes[1] as char;
    if a == 'U' || b == 'U' || xy == "AA" || xy == "DD" {
        return Some(GitDecor::Conflict);
    }
    if xy == "??" || xy == "!!" {
        return if xy == "??" {
            Some(GitDecor::Untracked)
        } else {
            None
        };
    }
    let added = a == 'A' || b == 'A';
    let changed =
        matches!(a, 'M' | 'D' | 'R' | 'T' | 'C') || matches!(b, 'M' | 'D' | 'R' | 'T' | 'C');
    if added && changed {
        return Some(GitDecor::Modified);
    }
    if added {
        return Some(GitDecor::Untracked);
    }
    if changed || a != ' ' || b != ' ' {
        return Some(GitDecor::Modified);
    }
    None
}

fn parse_porcelain_line(line: &str) -> Option<(GitDecor, String)> {
    if line.len() < 3 {
        return None;
    }
    let xy = &line[..2];
    let rest = line.get(3..).unwrap_or("").trim_start();
    let decor = classify_xy(xy)?;
    let path_part = if xy.contains('R') || xy.contains('C') {
        rest.rsplit_once(" -> ").map(|(_, p)| p).unwrap_or(rest)
    } else {
        rest
    };
    let rel = unquote_git_path(path_part);
    if rel.is_empty() {
        return None;
    }
    Some((decor, rel))
}

fn unquote_git_path(s: &str) -> String {
    let s = s.trim();
    let raw = if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        unescape_git(&s[1..s.len() - 1])
    } else {
        s.to_string()
    };
    raw.trim_end_matches('/').to_string()
}

fn unescape_git(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(o) => out.push(o),
                None => break,
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Immediate child directories of `listed_rel` that porcelain marks dirty.
///
/// `listed_rel` is the listed directory relative to the repo root (`""` when
/// listing the root). `child_dirs` are the immediate child directory names;
/// files are not colored.
pub fn child_dir_decors(
    listed_rel: &str,
    child_dirs: &[&str],
    porcelain: &str,
) -> BTreeMap<String, GitDecor> {
    let listed_rel = listed_rel.trim_matches('/');
    let mut out: BTreeMap<String, GitDecor> = BTreeMap::new();
    for line in porcelain.lines() {
        let Some((decor, path)) = parse_porcelain_line(line) else {
            continue;
        };
        let Some(child) = immediate_child(listed_rel, &path) else {
            continue;
        };
        if !child_dirs.iter().any(|dir| *dir == child) {
            continue;
        }
        out.entry(child)
            .and_modify(|old| *old = (*old).merge(decor))
            .or_insert(decor);
    }
    out
}

fn immediate_child(listed_rel: &str, path: &str) -> Option<String> {
    let rest = if listed_rel.is_empty() {
        path
    } else if path == listed_rel {
        return None;
    } else {
        let prefix = format!("{listed_rel}/");
        path.strip_prefix(&prefix)?
    };
    let child = rest.split('/').next().unwrap_or(rest);
    if child.is_empty() {
        None
    } else {
        Some(child.to_string())
    }
}

/// Worst decor in a nested repo's porcelain, merged with a color already
/// chosen for that child folder.
pub fn merge_repo_aggregate(
    existing: Option<GitDecor>,
    nested_porcelain: &str,
) -> Option<GitDecor> {
    let mut worst = existing;
    for line in nested_porcelain.lines() {
        if let Some((decor, _)) = parse_porcelain_line(line) {
            worst = GitDecor::merge_opt(worst, Some(decor));
        }
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn classify_conflict_untracked_modified() {
        assert_eq!(classify_xy("UU"), Some(GitDecor::Conflict));
        assert_eq!(classify_xy("AA"), Some(GitDecor::Conflict));
        assert_eq!(classify_xy("DU"), Some(GitDecor::Conflict));
        assert_eq!(classify_xy("??"), Some(GitDecor::Untracked));
        assert_eq!(classify_xy("A "), Some(GitDecor::Untracked));
        assert_eq!(classify_xy("AM"), Some(GitDecor::Modified));
        assert_eq!(classify_xy(" M"), Some(GitDecor::Modified));
        assert_eq!(classify_xy("M "), Some(GitDecor::Modified));
        assert_eq!(classify_xy("D "), Some(GitDecor::Modified));
        assert_eq!(classify_xy("!!"), None);
    }

    #[test]
    fn parse_rename_and_untracked_dir() {
        let (d, p) = parse_porcelain_line("?? newdir/").unwrap();
        assert_eq!(d, GitDecor::Untracked);
        assert_eq!(p, "newdir");
        let (d, p) = parse_porcelain_line("R  old.rs -> src/new.rs").unwrap();
        assert_eq!(d, GitDecor::Modified);
        assert_eq!(p, "src/new.rs");
    }

    #[test]
    fn rollup_child_when_listing_src() {
        let porcelain = " M src/sub/a.rs\n";
        let map = child_dir_decors("src", &["sub", "other"], porcelain);
        assert_eq!(map.get("sub"), Some(&GitDecor::Modified));
        assert!(map.get("other").is_none());
    }

    #[test]
    fn rollup_src_when_listing_repo_root() {
        let porcelain = " M src/sub/a.rs\n";
        let map = child_dir_decors("", &["src", "docs"], porcelain);
        assert_eq!(map.get("src"), Some(&GitDecor::Modified));
        assert!(map.get("docs").is_none());
    }

    #[test]
    fn untracked_dir_ignored_file_and_worst_wins() {
        let porcelain = "?? newdir/\n!! secret\n?? foo/a\n M foo/b\nUU foo/c\n M README.md\n";
        let map = child_dir_decors("", &["newdir", "secret", "foo", "src"], porcelain);
        assert_eq!(map.get("newdir"), Some(&GitDecor::Untracked));
        assert!(map.get("secret").is_none());
        assert_eq!(map.get("foo"), Some(&GitDecor::Conflict));
        assert!(map.get("src").is_none());
    }

    #[test]
    fn nested_repo_aggregate_beats_untracked() {
        assert_eq!(
            merge_repo_aggregate(Some(GitDecor::Untracked), " M inner.rs\n"),
            Some(GitDecor::Modified)
        );
        assert_eq!(merge_repo_aggregate(None, "!! ignored\n"), None);
    }

    #[test]
    fn shell_script_uses_gtk_files_colors() {
        assert!(LIB_SCRIPT.contains("38;2;59;130;246"));
        assert!(LIB_SCRIPT.contains("38;2;234;88;12"));
        assert!(LIB_SCRIPT.contains("38;2;220;38;38"));
    }

    #[test]
    fn shell_classify_matches_rust() {
        let cases = [
            "UU", "AA", "DD", "DU", "??", "A ", "AM", " M", "M ", "D ", "!!", "R ", "T ", "C ",
        ];
        let lib = scratch("classify").join("lib.bash");
        fs::write(&lib, LIB_SCRIPT).unwrap();
        for xy in cases {
            let out = Command::new("bash")
                .arg("-c")
                .arg(format!(
                    "source {}; printf '%s' \"$(gtk_term_classify_xy \"$1\")\"",
                    shell_quote(&lib)
                ))
                .arg("classify")
                .arg(xy)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let got = String::from_utf8_lossy(&out.stdout);
            let expect = classify_xy(xy).map(GitDecor::name).unwrap_or("");
            assert_eq!(got, expect, "xy={xy:?}");
        }
    }

    #[test]
    fn shell_rollup_matches_rust() {
        let root = scratch("rollup");
        let listed = root.join("src");
        fs::create_dir_all(listed.join("sub")).unwrap();
        fs::create_dir_all(listed.join("other")).unwrap();
        let lib = root.join("lib.bash");
        fs::write(&lib, LIB_SCRIPT).unwrap();
        let script = format!(
            r#"
source {lib}
declare -A COLORS
gtk_term_apply_porcelain {listed} {root} <<'EOF'
 M src/sub/a.rs
?? src/other/new
EOF
printf '%s\n' "${{COLORS[sub]}}" "${{COLORS[other]}}"
"#,
            lib = shell_quote(&lib),
            listed = shell_quote(&listed),
            root = shell_quote(&root),
        );
        let out = Command::new("bash").arg("-c").arg(script).output().unwrap();
        assert!(
            out.status.success(),
            "stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("modified"));
        assert_eq!(lines.next(), Some("untracked"));
    }

    #[test]
    fn shell_ls_colors_dirty_folders() {
        let root = scratch("ls");
        fs::create_dir_all(root.join("src/sub")).unwrap();
        fs::write(root.join("src/sub/a.rs"), "a\n").unwrap();
        fs::create_dir_all(root.join("clean")).unwrap();
        fs::create_dir_all(root.join("newdir")).unwrap();
        fs::write(root.join("newdir/f"), "n\n").unwrap();
        fs::create_dir_all(root.join("inner")).unwrap();
        fs::write(root.join("inner/tracked.rs"), "t\n").unwrap();

        git(&root, &["init", "-q"]);
        git(&root, &["add", "src/sub/a.rs"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        fs::write(root.join("src/sub/a.rs"), "changed\n").unwrap();

        git(&root.join("inner"), &["init", "-q"]);
        git(&root.join("inner"), &["add", "tracked.rs"]);
        git(&root.join("inner"), &["commit", "-q", "-m", "init"]);
        fs::write(root.join("inner/tracked.rs"), "changed\n").unwrap();

        let lib = root.join("lib.bash");
        fs::write(&lib, LIB_SCRIPT).unwrap();
        let out = Command::new("bash")
            .arg(&lib)
            .current_dir(&root)
            .env("TERM", "xterm-256color")
            .env_remove("NO_COLOR")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "status={:?} stderr={} stdout={}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        let text = String::from_utf8(out.stdout).unwrap();
        let orange = "\u{1b}[1;38;2;234;88;12m";
        let blue = "\u{1b}[1;38;2;59;130;246m";
        assert!(
            text.contains(&format!("{orange}src\u{1b}[0m")),
            "src not orange:\n{text:?}"
        );
        assert!(
            text.contains(&format!("{blue}newdir\u{1b}[0m")),
            "newdir not blue:\n{text:?}"
        );
        assert!(
            text.contains(&format!("{orange}inner\u{1b}[0m")),
            "nested repo not orange:\n{text:?}"
        );
        assert!(
            !text.contains(&format!("{orange}clean")) && !text.contains(&format!("{blue}clean")),
            "clean folder was recolored:\n{text:?}"
        );

        let sub = Command::new("bash")
            .arg(&lib)
            .arg("src")
            .current_dir(&root)
            .env("TERM", "xterm-256color")
            .env_remove("NO_COLOR")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .unwrap();
        let sub_text = String::from_utf8(sub.stdout).unwrap();
        assert!(
            sub_text.contains(&format!("{orange}sub\u{1b}[0m")),
            "listing src did not color sub:\n{sub_text:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bash_launch_uses_rcfile_and_zsh_uses_zdotdir() {
        let cache = scratch("install");
        let bash = install_into(&cache, Path::new("/bin/bash"), "bash").unwrap();
        assert_eq!(bash.argv[0], "/bin/bash");
        assert_eq!(bash.argv[1], "--rcfile");
        assert!(bash.argv[2].ends_with("git-ls.bashrc"));
        assert_eq!(bash.argv[3], "-i");
        let rc = fs::read_to_string(&bash.argv[3]).unwrap();
        assert!(rc.contains("/etc/bash.bashrc"));
        assert!(rc.contains(".bashrc"));
        assert!(bash.env.is_empty());

        let zsh = install_into(&cache, Path::new("/bin/zsh"), "zsh").unwrap();
        assert_eq!(zsh.argv, vec!["/bin/zsh".to_string(), "-i".into()]);
        assert!(zsh
            .env
            .iter()
            .any(|(k, v)| k == "ZDOTDIR" && v.ends_with("/zsh")));
        assert!(zsh.env.iter().any(|(k, _)| k == "GTK_TERM_USER_ZDOTDIR"));
        let _ = fs::remove_dir_all(&cache);
    }

    fn scratch(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "gtk-term-gitls-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-c")
            .arg("commit.gpgsign=false")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "gtk-term")
            .env("GIT_AUTHOR_EMAIL", "gtk-term@example.com")
            .env("GIT_COMMITTER_NAME", "gtk-term")
            .env("GIT_COMMITTER_EMAIL", "gtk-term@example.com")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
