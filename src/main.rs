use anyhow::{anyhow, Context, Result};
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use dialoguer::Confirm;
use git2::{Oid, Repository, Signature, Tree};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Parser, Debug)]
#[command(
    name = "bgit",
    version,
    about = "Commit specific files directly to a target branch without switching."
)]
struct Cli {
    /// Path to the repo (defaults to current dir or nearest discovered repo)
    #[arg(long)]
    repo: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Commit a file to a target branch without checking it out
    Commit {
        /// Target branch name (e.g. feature/foo)
        #[arg(long, value_name = "BRANCH")]
        branch: String,

        /// Commit message (defaults to "Update <path>")
        #[arg(short, long, value_name = "MSG")]
        message: Option<String>,

        /// Path to the file to commit (relative to repo root is recommended)
        #[arg(value_name = "PATH")]
        path: Utf8PathBuf,

        /// Push to origin after creating the commit (uses your local git auth)
        #[arg(long)]
        push: bool,

        /// If the branch doesn't exist locally, try to start from origin/BRANCH
        #[arg(long)]
        track_remote: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Commit {
            branch,
            message,
            path,
            push,
            track_remote,
        } => {
            commit_to_branch(
                cli.repo.as_deref(),
                &branch,
                &path,
                message.as_deref(),
                push,
                track_remote,
            )?;
        }
    }
    Ok(())
}

fn commit_to_branch(
    repo_hint: Option<&Path>,
    branch: &str,
    file_path: &Utf8PathBuf,
    msg_opt: Option<&str>,
    push: bool,
    track_remote: bool,
) -> Result<()> {
    // Open repo (discover upward if needed)
    let repo = match repo_hint {
        Some(p) => Repository::open(p)?,
        None => Repository::discover(".")?,
    };

    // Normalize to repo root
    let workdir = repo
        .workdir()
        .ok_or_else(|| anyhow!("Bare repositories are not supported"))?;
    let abs_file = if file_path.is_absolute() {
        PathBuf::from(file_path.as_str())
    } else {
        workdir.join(file_path.as_str())
    };
    if !abs_file.exists() {
        return Err(anyhow!("File not found: {}", abs_file.display()));
    }

    // Ensure branch exists (locally) or optionally track from origin
    let (parent_commit_oid, parent_tree_oid) = ensure_branch_base(&repo, branch, track_remote)
        .with_context(|| format!("Resolving base for branch '{branch}'"))?;

    let parent_commit = repo.find_commit(parent_commit_oid)?;
    let base_tree = repo.find_tree(parent_tree_oid)?;

    // Prepare blob for the file contents
    let blob_oid = repo.blob_path(&abs_file)?;

    // Determine filemode (executable vs normal)
    let meta = fs::metadata(&abs_file)?;
    let is_exec = (meta.permissions().mode() & 0o111) != 0;
    let filemode = if is_exec { 0o100755 } else { 0o100644 };

    // Compute repo-relative path for the tree entry
    let rel = path_relative_to(&abs_file, workdir).ok_or_else(|| {
        anyhow!(
            "Could not compute repo-relative path for {}",
            abs_file.display()
        )
    })?;

    // Build a new tree that is base_tree + (rel -> blob)
    let new_tree_oid = upsert_path_into_tree(&repo, &base_tree, rel.as_path(), blob_oid, filemode)?;
    let new_tree = repo.find_tree(new_tree_oid)?;

    // Author/committer: use git config
    let sig = Signature::now(&git_user_name(&repo)?, &git_user_email(&repo)?)?;

    let msg_default = format!("Update {}", rel.display());
    let message = msg_opt.unwrap_or(&msg_default);

    // Create commit and move the branch ref
    let refname = format!("refs/heads/{branch}");
    let new_commit_oid = repo.commit(
        Some(&refname),
        &sig,
        &sig,
        message,
        &new_tree,
        &[&parent_commit],
    )?;

    println!(
        "✅ Committed {} to {}\n   commit {}",
        rel.display(),
        branch,
        new_commit_oid
    );

    if push {
        // Use user's CLI for auth to avoid libgit2 credential plumbing
        let status = Command::new("git")
            .arg("-C")
            .arg(workdir)
            .arg("push")
            .arg("origin")
            .arg(format!("{0}:{0}", branch))
            .status()
            .context("Failed to run 'git push'")?;
        if !status.success() {
            return Err(anyhow!("git push failed"));
        }
        println!("🚀 Pushed {branch} to origin");
    }

    Ok(())
}

fn git_user_name(repo: &Repository) -> Result<String> {
    let cfg = repo.config()?;
    let name = cfg
        .get_string("user.name")
        .or_else(|_| git2::Config::open_default()?.get_string("user.name"))?;
    Ok(name)
}

fn git_user_email(repo: &Repository) -> Result<String> {
    let cfg = repo.config()?;
    let email = cfg
        .get_string("user.email")
        .or_else(|_| git2::Config::open_default()?.get_string("user.email"))?;
    Ok(email)
}

fn ensure_branch_base(repo: &Repository, branch: &str, track_remote: bool) -> Result<(Oid, Oid)> {
    // Try local branch first
    if let Ok(reference) = repo.find_reference(&format!("refs/heads/{branch}")) {
        let oid = reference
            .target()
            .ok_or_else(|| anyhow!("Invalid ref for {branch}"))?;
        let commit = repo.find_commit(oid)?;
        let tree = commit.tree()?;
        return Ok((oid, tree.id()));
    }

    // Optionally try origin/branch as the starting point
    if track_remote {
        // Make sure we have the latest refs
        let _ = Command::new("git")
            .arg("-C")
            .arg(repo.workdir().unwrap())
            .arg("fetch")
            .arg("origin")
            .status();

        let remote_ref = format!("refs/remotes/origin/{branch}");
        if let Ok(reference) = repo.find_reference(&remote_ref) {
            let target_oid = reference
                .target()
                .ok_or_else(|| anyhow!("Remote ref has no target: {remote_ref}"))?;
            let target_commit = repo.find_commit(target_oid)?;
            // Create a local branch at this commit so later commit() can update it
            repo.branch(branch, &target_commit, false)?;
            let tree = target_commit.tree()?;
            return Ok((target_commit.id(), tree.id()));
        }
    }

    // Branch doesn't exist - prompt user to create it
    let create = Confirm::new()
        .with_prompt(format!(
            "Branch '{}' does not exist. Create it from HEAD?",
            branch
        ))
        .default(true)
        .interact()?;

    if !create {
        return Err(anyhow!(
            "Branch '{branch}' not found locally{}",
            if track_remote { " or on origin" } else { "" }
        ));
    }

    // Create the branch from HEAD
    let head = repo.head()?;
    let head_commit = head.peel_to_commit()?;
    repo.branch(branch, &head_commit, false)?;

    let tree = head_commit.tree()?;
    println!("✨ Created new branch '{}' from HEAD", branch);

    Ok((head_commit.id(), tree.id()))
}

/// Return path a relative to base, if possible
fn path_relative_to<'a>(a: &Path, base: &Path) -> Option<PathBuf> {
    let areal = a.canonicalize().ok()?;
    let baser = base.canonicalize().ok()?;
    pathdiff::diff_paths(areal, baser)
}

/// Recursively upsert a file at `path` with `blob_oid` into `base_tree`, returning the new root tree oid.
fn upsert_path_into_tree(
    repo: &Repository,
    base_tree: &Tree,
    path: &Path,
    blob_oid: Oid,
    filemode: i32,
) -> Result<Oid> {
    let comps: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();

    let mut comps: Vec<&str> = comps.iter().map(|s| s.as_str()).collect();

    if comps.is_empty() {
        return Err(anyhow!("Empty path"));
    }
    let new_tree_oid =
        upsert_components(repo, Some(base_tree), &mut comps[..], blob_oid, filemode)?;
    Ok(new_tree_oid)
}

fn upsert_components<'a>(
    repo: &Repository,
    base_tree: Option<&Tree<'a>>,
    comps: &mut [&str],
    blob_oid: Oid,
    filemode: i32,
) -> Result<Oid> {
    if comps.is_empty() {
        return Err(anyhow!("No components"));
    }

    // If last component -> insert blob here
    if comps.len() == 1 {
        let name = comps[0];
        let mut tb = match base_tree {
            Some(t) => repo.treebuilder(Some(t))?,
            None => repo.treebuilder(None)?,
        };
        tb.insert(name, blob_oid, filemode)?;
        return Ok(tb.write()?);
    }

    // Otherwise, handle directory
    let name = comps[0];
    let rest = &mut comps[1..];

    // Find existing subtree (if any)
    let mut existing_subtree_oid: Option<Oid> = None;
    if let Some(t) = base_tree {
        if let Some(entry) = t.get_name(name) {
            existing_subtree_oid = entry
                .to_object(repo)
                .ok()
                .and_then(|o| o.as_tree().map(|t| t.id()));
        }
    }

    let existing_subtree = match existing_subtree_oid {
        Some(oid) => Some(repo.find_tree(oid)?),
        None => None,
    };
    let child_oid = upsert_components(repo, existing_subtree.as_ref(), rest, blob_oid, filemode)?;

    // Rebuild current level with updated subtree
    let mut tb = match base_tree {
        Some(t) => repo.treebuilder(Some(t))?,
        None => repo.treebuilder(None)?,
    };
    tb.insert(name, child_oid, 0o040000)?;
    Ok(tb.write()?)
}
