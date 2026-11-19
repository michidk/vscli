use color_eyre::eyre::{Result, WrapErr, bail};
use std::path::{Path, PathBuf};

#[derive(Debug)]
enum CopyKind {
    Directory(Vec<CopyPlan>),
    File,
}

#[derive(Debug)]
pub(super) struct CopyPlan {
    source: PathBuf,
    destination: PathBuf,
    kind: CopyKind,
}

impl CopyPlan {
    fn build(source: PathBuf, destination: PathBuf) -> Result<Self> {
        if destination.exists() {
            bail!(
                "Refusing to overwrite existing path: {}",
                destination.display()
            );
        }

        let kind = if source.is_dir() {
            CopyKind::Directory(Self::children(&source, &destination)?)
        } else if source.is_file() {
            CopyKind::File
        } else {
            bail!("Unsupported config entry: {}", source.display());
        };

        Ok(Self {
            source,
            destination,
            kind,
        })
    }

    pub(super) fn children(source: &Path, destination: &Path) -> Result<Vec<Self>> {
        std::fs::read_dir(source)
            .wrap_err_with(|| format!("Failed to read source directory: {}", source.display()))?
            .map(|entry| {
                let entry = entry.wrap_err("Failed to read source directory entry")?;
                Self::build(entry.path(), destination.join(entry.file_name()))
            })
            .collect()
    }

    pub(super) fn execute(self) -> Result<()> {
        match self.kind {
            CopyKind::Directory(children) => {
                std::fs::create_dir_all(&self.destination).wrap_err_with(|| {
                    format!(
                        "Failed to create destination directory: {}",
                        self.destination.display()
                    )
                })?;

                for child in children {
                    child.execute()?;
                }
            }
            CopyKind::File => {
                std::fs::copy(&self.source, &self.destination).wrap_err_with(|| {
                    format!(
                        "Failed to copy file from {} to {}",
                        self.source.display(),
                        self.destination.display()
                    )
                })?;
            }
        }

        Ok(())
    }
}
