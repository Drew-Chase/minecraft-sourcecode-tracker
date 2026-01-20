use anyhow::Result;
use git2::{Cred, PushOptions, RemoteCallbacks, Repository};
use log::{debug, info, warn};
use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

pub struct GitTracker {
    pub repository: Repository,
    username: String,
    auth_token: String,
}

impl GitTracker {
    pub fn new(
        git_dir: impl AsRef<Path>,
        repo_dir: impl AsRef<Path>,
        url: impl AsRef<str>,
        username: impl AsRef<str>,
        auth_token: impl AsRef<str>,
        email: Option<String>,
    ) -> Result<Self> {
        let git_dir = git_dir.as_ref();
        let repo_dir = repo_dir.as_ref();
        let url = url.as_ref();
        let username = username.as_ref().to_string();
        let auth_token = auth_token.as_ref().to_string();

        // Try to open existing repository, or initialize a new one
        let repository = match Repository::open(git_dir) {
            Ok(repo) => {
                debug!("Opened existing repository at {:?}", git_dir);
                repo
            }
            Err(_) => {
                debug!("Initializing new repository at {:?}", git_dir);
                let repo = Repository::init(git_dir)?;

                // Add origin remote (only for new repos)
                repo.remote("origin", url)?;
                repo.remote_set_pushurl("origin", Some(url))?;

                repo
            }
        };

        // Set the workdir (always, in case it changed)
        repository.set_workdir(repo_dir, false)?;

        // Set local git config for user.name and user.email
        {
            let mut config = repository.config()?;
            config.set_str("user.name", &username)?;
            let default_email = format!("{}@users.noreply.github.com", &username);
            config.set_str("user.email", email.as_deref().unwrap_or(&default_email))?;
        }

        Ok(GitTracker {
            repository,
            username,
            auth_token,
        })
    }

    pub fn create_commit(
        &'_ self,
        directory: impl AsRef<Path>,
        commit_message: impl AsRef<str>,
        branch: impl AsRef<str>,
        tag_name: Option<String>,
    ) -> Result<()> {
        let directory = directory.as_ref().to_path_buf();
        let branch = branch.as_ref();
        let commit_message = commit_message.as_ref();
        info!("Creating commit {}", commit_message);

        let mut index = self.repository.index()?;

        debug!("Clearing index!");
        index.clear()?;

        debug!("Adding all files form the working directory to the index");
        self.add_directory_to_index(&mut index, &directory, &directory)?;

        debug!("Writing all files to the index");
        index.write()?;

        debug!("Creating tree from the index");
        let tree_id = index.write_tree()?;
        debug!("Created tree with id {}", tree_id);

        let tree = self.repository.find_tree(tree_id)?;
        let head = self.repository.head();

        let parent_commit = match head {
            Ok(head) => {
                debug!("Parent commit: {:?}", head.peel_to_commit());
                Some(head.peel_to_commit()?)
            }
            Err(_) => {
                warn!(
                    r#"
                        No parent commit was present,
                        this could either mean that this is the first commit
                        or that the repository is in an inconsistent state.
                    "#
                );
                None
            }
        };

        let parents = match &parent_commit {
            Some(parent) => {
                debug!("Using parent commit: {}", parent.id());
                vec![parent]
            }
            None => {
                vec![]
            }
        };

        debug!("Getting repository signature");
        let sig = self.repository.signature()?;
        debug!(
            "Signature: {} <{}>",
            sig.name().unwrap_or("Unknown"),
            sig.email().unwrap_or("Unknown")
        );

        let commit_id =
            self.repository
                .commit(Some("HEAD"), &sig, &sig, commit_message, &tree, &parents)?;

        debug!("Commit created: {}", commit_id);

        // Create or update the branch to point to the new commit
        // Skip if HEAD is already pointing to this branch (commit already updated it)
        let commit = self.repository.find_commit(commit_id)?;
        let head_is_branch = self
            .repository
            .head()
            .ok()
            .and_then(|h| h.shorthand().map(|s| s == branch))
            .unwrap_or(false);

        if head_is_branch {
            debug!(
                "HEAD is already '{}', branch updated via commit to {}",
                branch, commit_id
            );
        } else {
            self.repository.branch(branch, &commit, true)?; // true = force update if exists
            debug!(
                "Branch '{}' created/updated to point to {}",
                branch, commit_id
            );
        }

        // Create tag if provided
        if let Some(ref tag) = tag_name {
            debug!("Creating tag: {}", tag);
            let commit_obj = self
                .repository
                .find_object(commit_id, Some(git2::ObjectType::Commit))?;

            // Create an annotated tag (recommended)
            self.repository.tag(
                tag,
                &commit_obj,
                &sig,
                &format!("Tag for commit {}", commit_id),
                false, // don't force to overwrite an existing tag
            )?;
            debug!("Tag '{}' created successfully", tag);
        }

        debug!("Pushing to remote");
        let mut remote = self.repository.find_remote("origin")?;

        // Build refspecs
        let mut refspecs = vec![format!(
            "refs/heads/{branch}:refs/heads/{branch}",
            branch = branch
        )];

        if let Some(ref tag) = tag_name {
            refspecs.push(format!("refs/tags/{tag}:refs/tags/{tag}"));
        }

        // Set up authentication callbacks
        let mut callbacks = RemoteCallbacks::new();
        let username = self.username.clone();
        let auth_token = self.auth_token.clone();
        let attempts = Cell::new(0u32);

        callbacks.credentials(move |url, username_from_url, allowed_types| {
            let attempt = attempts.get() + 1;
            attempts.set(attempt);
            debug!(
                "Credentials callback attempt {}: url={}, username_from_url={:?}, allowed_types={:?}",
                attempt, url, username_from_url, allowed_types
            );
            // Allow a few attempts for multi-refspec pushes, but prevent infinite loops
            if attempt > 3 {
                warn!("Too many credential attempts, authentication likely failing");
                return Err(git2::Error::from_str("authentication failed after multiple attempts"));
            }
            Cred::userpass_plaintext(&username, &auth_token)
        });

        // Accept all certificates (needed for self-hosted git servers with custom CAs)
        callbacks.certificate_check(|_cert, _host| Ok(git2::CertificateCheckStatus::CertificateOk));

        // Track push errors from the remote
        let push_error: std::sync::Arc<std::sync::Mutex<Option<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let push_error_clone = push_error.clone();

        callbacks.push_update_reference(move |refname, status| {
            debug!("Push update reference: refname={}, status={:?}", refname, status);
            if let Some(msg) = status {
                warn!("Push rejected for {}: {}", refname, msg);
                if let Ok(mut err) = push_error_clone.lock() {
                    *err = Some(format!("Push rejected for {}: {}", refname, msg));
                }
            }
            Ok(())
        });

        callbacks.push_transfer_progress(|current, total, bytes| {
            debug!(
                "Push progress: {}/{} objects, {} bytes",
                current, total, bytes
            );
        });

        let mut push_options = PushOptions::new();
        push_options.remote_callbacks(callbacks);

        let refspec_strs: Vec<&str> = refspecs.iter().map(|s| s.as_str()).collect();
        remote.push(&refspec_strs, Some(&mut push_options))?;

        // Check if the remote rejected any refs
        if let Ok(err) = push_error.lock()
            && let Some(ref msg) = *err
        {
            return Err(anyhow::anyhow!("{}", msg));
        }

        let remote_url = remote.url().unwrap_or("");
        info!(
            "Commit created and pushed successfully with id: {commit_id} and can be viewed at {url}/tree/{commit_id}",
            commit_id = commit_id,
            url = remote_url
        );

        Ok(())
    }

    /// Helper function to recursively add files from a directory to the git index
    #[allow(clippy::only_used_in_recursion)]
    fn add_directory_to_index(
        &self,
        index: &mut git2::Index,
        dir_path: &PathBuf,
        base_path: &PathBuf,
    ) -> Result<()> {
        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let path = entry.path();

            let file_type = entry.file_type()?;

            if let Some(filename) = path.file_name()
                && filename.eq("summary.txt")
            {
                continue;
            }

            if file_type.is_dir() {
                // Recursively add subdirectory
                self.add_directory_to_index(index, &path, base_path)?;
            } else if file_type.is_file() {
                // Calculate the relative path from base_path
                let relative_path = path.strip_prefix(base_path)?;
                debug!("Adding file to index: {:?}", relative_path);
                index.add_path(relative_path)?;
            }
        }
        Ok(())
    }

    /// Fetches a branch from the remote and sets up the local branch to track it.
    /// This must be called before create_commit to ensure the new commit is built
    /// on top of the existing remote history.
    pub fn fetch_branch(&self, branch: impl AsRef<str>) -> Result<()> {
        let branch = branch.as_ref();
        debug!("Fetching branch '{}' from remote", branch);

        let mut remote = self.repository.find_remote("origin")?;

        // Set up authentication callbacks
        let mut callbacks = RemoteCallbacks::new();
        let username = self.username.clone();
        let auth_token = self.auth_token.clone();
        let attempts = Cell::new(0u32);

        callbacks.credentials(move |url, username_from_url, allowed_types| {
            let attempt = attempts.get() + 1;
            attempts.set(attempt);
            debug!(
                "Credentials callback (fetch branch) attempt {}: url={}, username_from_url={:?}, allowed_types={:?}",
                attempt, url, username_from_url, allowed_types
            );
            if attempt > 3 {
                warn!("Too many credential attempts, authentication likely failing");
                return Err(git2::Error::from_str("authentication failed after multiple attempts"));
            }
            Cred::userpass_plaintext(&username, &auth_token)
        });

        callbacks.certificate_check(|_cert, _host| Ok(git2::CertificateCheckStatus::CertificateOk));

        // Set up fetch options
        let mut fetch_options = git2::FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

        // Fetch the specific branch
        let refspec = format!("refs/heads/{branch}:refs/remotes/origin/{branch}");
        debug!("Fetching with refspec: {}", refspec);

        match remote.fetch(&[&refspec], Some(&mut fetch_options), None) {
            Ok(_) => debug!("Fetch completed successfully"),
            Err(e) => {
                // If the branch doesn't exist on the remote yet, that's OK - it's a new branch
                if e.code() == git2::ErrorCode::NotFound {
                    info!(
                        "Branch '{}' does not exist on remote yet, will create it",
                        branch
                    );
                    return Ok(());
                }
                return Err(e.into());
            }
        }

        // Check if we got the remote branch
        let remote_ref_name = format!("refs/remotes/origin/{}", branch);
        match self.repository.find_reference(&remote_ref_name) {
            Ok(remote_ref) => {
                let remote_commit = remote_ref.peel_to_commit()?;
                debug!(
                    "Remote branch '{}' points to commit {}",
                    branch,
                    remote_commit.id()
                );

                // Check if HEAD is already pointing to this branch
                let head_is_branch = self
                    .repository
                    .head()
                    .ok()
                    .and_then(|h| h.shorthand().map(|s| s == branch))
                    .unwrap_or(false);

                if head_is_branch {
                    // HEAD is already on this branch - check if we need to update it
                    let head_commit = self.repository.head()?.peel_to_commit()?;
                    if head_commit.id() == remote_commit.id() {
                        debug!(
                            "HEAD is already on '{}' at commit {}, no update needed",
                            branch,
                            head_commit.id()
                        );
                    } else {
                        // Local is different from remote - reset HEAD to remote commit
                        debug!(
                            "Resetting HEAD from {} to remote commit {}",
                            head_commit.id(),
                            remote_commit.id()
                        );
                        self.repository.reset(
                            remote_commit.as_object(),
                            git2::ResetType::Soft,
                            None,
                        )?;
                    }
                } else {
                    // HEAD is not on this branch - create/update branch and switch to it
                    self.repository.branch(branch, &remote_commit, true)?;
                    debug!(
                        "Local branch '{}' updated to point to {}",
                        branch,
                        remote_commit.id()
                    );

                    self.repository
                        .set_head(&format!("refs/heads/{}", branch))?;
                    debug!("HEAD set to refs/heads/{}", branch);
                }
            }
            Err(e) => {
                // Branch might not exist on remote yet
                debug!(
                    "Could not find remote reference '{}': {} - branch may be new",
                    remote_ref_name, e
                );
            }
        }

        Ok(())
    }

    /// Fetches tags from the remote and returns them
    pub fn get_remote_tags(&self) -> Result<Vec<String>> {
        let mut remote = self.repository.find_remote("origin")?;

        // Set up authentication callbacks
        let mut callbacks = RemoteCallbacks::new();
        let username = self.username.clone();
        let auth_token = self.auth_token.clone();
        let attempts = Cell::new(0u32);

        callbacks.credentials(move |url, username_from_url, allowed_types| {
            let attempt = attempts.get() + 1;
            attempts.set(attempt);
            debug!(
                "Credentials callback (fetch) attempt {}: url={}, username_from_url={:?}, allowed_types={:?}",
                attempt, url, username_from_url, allowed_types
            );
            if attempt > 3 {
                warn!("Too many credential attempts, authentication likely failing");
                return Err(git2::Error::from_str("authentication failed after multiple attempts"));
            }
            Cred::userpass_plaintext(&username, &auth_token)
        });

        // Accept all certificates (needed for self-hosted git servers with custom CAs)
        callbacks.certificate_check(|_cert, _host| Ok(git2::CertificateCheckStatus::CertificateOk));

        // Connect to the remote
        remote.connect_auth(git2::Direction::Fetch, Some(callbacks), None)?;

        // Get the list of remote references
        let refs = remote.list()?;

        let tags: Vec<String> = refs
            .iter()
            .filter_map(|r| {
                let name = r.name();
                // Remote tags are listed as "refs/tags/tag_name"
                if name.starts_with("refs/tags/") {
                    Some(name.trim_start_matches("refs/tags/").to_string())
                } else {
                    None
                }
            })
            .collect();

        remote.disconnect()?;

        Ok(tags)
    }
}
