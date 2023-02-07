use crate::backend::project::Project;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::task::LocalPoolHandle;
use tower_lsp::lsp_types::{CodeLens, WorkspaceFolder};
use tower_lsp::Client;
use url::Url;

fn as_path(url: &Url) -> Option<&Path> {
    if url.scheme() == "file" {
        Some(Path::new(url.path()))
    } else {
        None
    }
}

/// A workspace, managing attached folders
pub struct Workspace {
    inner: Arc<RwLock<Inner>>,
}

impl Workspace {
    pub fn new(client: Client) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner::new(client))),
        }
    }

    pub async fn folders_changed(
        &self,
        added: Vec<WorkspaceFolder>,
        removed: Vec<WorkspaceFolder>,
    ) {
        self.inner
            .write()
            .await
            .folders_changed(added, removed)
            .await
    }

    pub async fn changed(&self, path: &Url) {
        self.inner.write().await.changed(path).await;
    }

    pub async fn code_lens(&self, path: &Url) -> anyhow::Result<Vec<CodeLens>> {
        self.inner.read().await.code_lens(path).await
    }
}

struct Inner {
    client: Client,
    folders: HashMap<PathBuf, Folder>,
    pool: LocalPoolHandle,
}

impl Inner {
    pub fn new(client: Client) -> Self {
        Self {
            pool: LocalPoolHandle::new(10),
            client,
            folders: Default::default(),
        }
    }

    pub async fn folders_changed(
        &mut self,
        added: Vec<WorkspaceFolder>,
        removed: Vec<WorkspaceFolder>,
    ) {
        for path in removed {
            if let Some(path) = as_path(&path.uri) {
                self.folders.remove(path);
            }
        }

        for path in added {
            if let Some(path) = as_path(&path.uri) {
                self.folders.insert(
                    path.to_path_buf(),
                    Folder::new(self.client.clone(), path.into(), self.pool.clone()).await,
                );
            }
        }

        log::info!("New workspace set: {:?}", self.folders);
    }

    pub async fn changed(&mut self, path: &Url) {
        if let Some(path) = as_path(path) {
            for (root, folder) in &mut self.folders {
                if path.starts_with(root) {
                    folder.changed(path).await;
                }
            }
        }
    }

    pub async fn code_lens(&self, path: &Url) -> anyhow::Result<Vec<CodeLens>> {
        let mut result = vec![];

        if let Some(path) = as_path(path) {
            for (root, folder) in &self.folders {
                if path.starts_with(root) {
                    result.extend(folder.code_lens(path).await?);
                }
            }
        }

        Ok(result)
    }
}

/// A workspace folder
#[derive(Debug)]
pub struct Folder {
    client: Client,
    root: PathBuf,
    pool: LocalPoolHandle,
    projects: HashMap<PathBuf, Project>,
}

impl Folder {
    pub async fn new(client: Client, root: PathBuf, pool: LocalPoolHandle) -> Self {
        let mut result = Self {
            client,
            root,
            pool,
            projects: Default::default(),
        };
        result.scan().await;
        result
    }

    /// Do a full scan
    async fn scan(&mut self) {
        for entry in walkdir::WalkDir::new(&self.root)
            .into_iter()
            .filter_entry(|d| d.file_type().is_dir())
        {
            if let Ok(entry) = entry {
                self.eval(entry.path()).await;
            }
        }
    }

    /// Some file in the folder structure changed
    pub async fn changed(&mut self, path: &Path) {
        log::info!("File changed: {}", path.display());

        let mut to_remove = vec![];

        for (k, v) in &mut self.projects {
            if path.starts_with(k) {
                if !Project::has_marker(k) {
                    // drop
                    to_remove.push(k.to_path_buf());
                } else {
                    // changed
                    v.changed(path).await;
                }
            }
        }

        for k in to_remove {
            self.projects.remove(&k);
        }
    }

    async fn eval(&mut self, path: &Path) {
        let key = path.to_path_buf();
        match (self.projects.entry(key), Project::has_marker(path)) {
            (Entry::Vacant(entry), true) => {
                // add
                log::info!("Add new project: {}", path.display());
                entry.insert(
                    Project::new(self.client.clone(), path.into(), self.pool.clone()).await,
                );
            }
            (Entry::Occupied(entry), false) => {
                // remove
                log::info!("Remove project: {}", path.display());
                entry.remove();
            }
            _ => {
                // just a change
            }
        }
    }

    pub async fn code_lens(&self, path: &Path) -> anyhow::Result<Vec<CodeLens>> {
        let mut result = vec![];
        for (k, v) in &self.projects {
            if path.starts_with(k) {
                result.extend(v.code_lens(path).await?);
            }
        }
        Ok(result)
    }
}
