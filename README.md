# bgit 🪄

**bgit** is a Rust CLI tool that lets you commit specific files directly to target branches **without switching branches**.

This tool is built on top of [libgit2](https://libgit2.org/) via the [`git2` crate](https://crates.io/crates/git2), providing a fast and native way to automate cross-branch commits in Git repositories.

---

## ✨ Features

- 🔀 Commit to *any* branch without checkout
- 🧭 Auto-discovers your current repository
- 🪶 Works from your normal working tree (no detached HEADs)
- 🌱 Can start a new branch from `origin/<branch>` with `--track-remote`
- 🚀 Optional `--push` support (delegates to `git push` for normal authentication)
- 🔒 Safe: commits are atomic and branch-local

---

## 🧩 Installation

Install directly from [crates.io](https://crates.io):

```bash
cargo install bgit
```

Or build from source:

```bash
git clone https://github.com/yourusername/bgit.git
cd bgit
cargo build --release
cp target/release/bgit ~/.local/bin/
```

---

## 🧠 Usage

### Commit a file directly to a branch
```bash
bgit commit --branch feature/ui src/ui.rs
```

### With a custom message
```bash
bgit commit --branch hotfix/login src/login.rs -m "Fix login redirect"
```

### Push immediately after committing
```bash
bgit commit --branch release/1.2.3 dist/app.js --push
```

### Track a remote branch if missing
If the branch doesn’t exist locally, use `--track-remote` to seed it from `origin/BRANCH`:

```bash
bgit commit --branch feature/experimental new/feature.rs --track-remote
```

---

## ⚙️ Command Reference
```
bgit commit \
  --branch <BRANCH> \
  [--message <MSG>] \
  [--push] \
  [--track-remote] \
  [--repo <PATH>] \
  <FILE>
```

### Flags
| Flag | Description |
|------|--------------|
| `--branch` | Target branch to commit to |
| `--message`, `-m` | Custom commit message (default: `Update <file>`) |
| `--push` | Push the branch to origin after committing |
| `--track-remote` | Create local branch from remote if missing |
| `--repo` | Optional path to Git repo (auto-detects by default) |

---

## 🧱 How It Works

1. Opens or discovers the Git repository.
2. Finds the target branch (creates it from remote if needed).
3. Hashes the provided file into a blob.
4. Rebuilds the target branch tree with that file.
5. Creates a new commit object pointing to that tree.
6. Optionally calls `git push` to sync the branch to origin.

All without switching branches or touching your working index.

---

## ⚠️ Notes & Limitations

- Does **not** run Git hooks (like `pre-commit`) — since it bypasses normal checkout.
- Does **not** merge; it only commits changes directly to the branch head.
- Assumes your repository is non-bare and accessible.
- To avoid overwriting concurrent changes, you should rebase or pull the target branch before running this tool.

---

## 🧪 Example Workflow

```bash
# Commit fileA to branchA
bgit commit --branch branchA fileA.txt -m "Update A"

# Commit fileB to branchB
bgit commit --branch branchB fileB.txt -m "Update B"

# Push both
bgit commit --branch branchA fileA.txt --push
bgit commit --branch branchB fileB.txt --push
```

---

## 🔮 Roadmap

- [ ] Multi-file commits per branch
- [ ] Bulk mode: `bgit commit --map "a.txt:branchA" "b.txt:branchB"`
- [ ] Hook-aware mode using temporary worktrees
- [ ] Dry-run / diff preview mode

---

## 🪪 License

MIT © 2025 [Your Name](https://github.com/yourusername)

---

## 🤝 Contributing

Pull requests are welcome! If you’d like to add features or improve UX, fork the repo and submit a PR.

```bash
git clone https://github.com/yourusername/bgit.git
cd bgit
cargo run -- commit --branch test examples/demo.txt
```
