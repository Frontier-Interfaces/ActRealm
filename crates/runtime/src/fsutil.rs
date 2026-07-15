use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;

pub(crate) fn ensure_private_directory(path: &Path) -> io::Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    let mut builder = DirBuilder::new();
    builder.recursive(true).mode(0o700).create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(true)
}
