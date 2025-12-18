use anyhow::{Result, anyhow, bail};
use git2::{Cred, PushOptions, RemoteCallbacks, Repository, RepositoryInitOptions};
use log::{debug, info, warn};
use std::fs;
use std::path::PathBuf;

pub struct GitTracker {
    pub repository: Repository,
    username: String,
    auth_token: String,
}

impl GitTracker {
    pub fn new(
        path: impl AsRef<str>,
        url: impl AsRef<str>,
        username: impl AsRef<str>,
        auth_token: impl AsRef<str>,
        email: Option<String>,
    ) -> Result<Self> {
        let path = path.as_ref();
        let url = url.as_ref();
        let username = username.as_ref().to_string();
        let auth_token = auth_token.as_ref().to_string();

        let mut options = RepositoryInitOptions::new();
        options.workdir_path(std::path::Path::new(path));
        options.no_dotgit_dir(true);
        options.origin_url(url);

        let repository = Repository::init_opts(path, &options)?;
        repository.remote_set_pushurl("origin", Some(url))?;

        // Set local git config for user.name and user.email
        {
            let mut config = repository.config()?;
            config.set_str("user.name", &username)?;
            if let Some(ref email) = email {
                config.set_str("user.email", email)?;
            }
        }

        Ok(GitTracker {
            repository,
            username,
            auth_token,
        })
    }

    pub fn create_commit(
        &'_ self,
        commit_message: impl AsRef<str>,
        branch: impl AsRef<str>,
        tag_name: Option<String>,
    ) -> Result<()> {
        let branch = branch.as_ref();
        let commit_message = commit_message.as_ref();
        info!("Creating commit {}", commit_message);
        let working_dir = self
            .repository
            .workdir()
            .ok_or_else(|| anyhow!("Failed to get working directory"))?
            .to_path_buf();

        let mut index = self.repository.index()?;

        debug!("Clearing index!");
        index.clear()?;

        debug!("Adding all files form the working directory to the index");
        self.add_directory_to_index(&mut index, &working_dir, &working_dir)?;

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

        // Create a tag if provided
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
        
        callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
            Cred::userpass_plaintext(&username, &auth_token)
        });

        let mut push_options = PushOptions::new();
        push_options.remote_callbacks(callbacks);

        let refspec_strs: Vec<&str> = refspecs.iter().map(|s| s.as_str()).collect();
        remote.push(&refspec_strs, Some(&mut push_options))?;

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

    pub fn get_tags(&self) -> Result<Vec<String>> {
        match self.repository.tag_names(None) {
            Ok(tags) => Ok(tags
                .into_iter()
                .filter_map(|tag| tag.map(|tag| tag.trim().to_string()))
                .collect::<Vec<String>>()),
            Err(_) => bail!("Could not get tags"),
        }
    }
}
