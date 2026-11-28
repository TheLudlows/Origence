use crate::service::Service;
use anyhow::{Context, ensure};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
};
use uuid::Uuid;

const ENV: &str = "OC_DEPLOYMENT_ADMIN_KEY";
const FILE: &str = "deployment-admin.key";
const KEY_LEN: usize = 73;

/// Register the first administrator before the listener opens. The authority
/// holds its data-directory lock throughout. Existing credentials are retained.
pub async fn initialize_deployment_admin(service: &Service) -> anyhow::Result<()> {
    let store = service.engine.relational();
    if store.has_deployment_admin().await? {
        return Ok(());
    }
    let supplied = match std::env::var(ENV) {
        Ok(key) if !key.is_empty() => Some(key),
        Ok(_) | Err(std::env::VarError::NotPresent) => None,
        Err(_) => anyhow::bail!("{ENV} is invalid"),
    };
    let key = if let Some(key) = supplied {
        key
    } else {
        let dir = store
            .path()
            .parent()
            .context("invalid authority path")?
            .to_owned();
        tokio::task::spawn_blocking(move || load_or_create(&dir)).await??
    };
    validate(&key)?;
    store.initialize_deployment_admin(&key).await?;
    Ok(())
}

fn validate(key: &str) -> anyhow::Result<()> {
    ensure!(
        key.strip_prefix("oc_admin_").is_some_and(|secret| {
            key.len() == KEY_LEN
                && secret
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        }),
        "invalid deployment administrator key format"
    );
    Ok(())
}

fn load_or_create(dir: &Path) -> anyhow::Result<String> {
    let path = dir.join(FILE);
    match fs::symlink_metadata(&path) {
        Ok(info) => {
            ensure!(
                info.is_file(),
                "invalid local deployment administrator key file"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                ensure!(
                    info.permissions().mode() & 0o077 == 0,
                    "deployment administrator key file is not private"
                );
            }
            let mut key = String::new();
            File::open(&path)?
                .take((KEY_LEN + 2) as u64)
                .read_to_string(&mut key)?;
            return Ok(key.strip_suffix('\n').unwrap_or(&key).to_owned());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    let key = format!(
        "oc_admin_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let staged = dir.join(format!(".deployment-admin-{}.tmp", Uuid::new_v4()));
    let mut file =
        private_create(&staged).context("cannot create deployment administrator key file")?;
    let publish = (|| -> io::Result<()> {
        writeln!(file, "{key}")?;
        file.sync_all()?;
        drop(file);
        publish_key(dir, &staged, &path)
    })();
    if publish.is_err() {
        let _ = fs::remove_file(&staged);
    }
    publish.context("cannot publish deployment administrator key file")?;
    Ok(key)
}

#[cfg(unix)]
fn private_create(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(unix)]
fn publish_key(dir: &Path, staged: &Path, path: &Path) -> io::Result<()> {
    fs::rename(staged, path)?;
    File::open(dir)?.sync_all()
}

#[cfg(windows)]
fn publish_key(_dir: &Path, staged: &Path, path: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    let staged = wide_path(staged);
    let path = wide_path(path);
    // Both NUL-terminated paths remain valid for the duration of the call.
    if unsafe { MoveFileExW(staged.as_ptr(), path.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain([0]).collect()
}

#[cfg(windows)]
fn private_create(path: &Path) -> io::Result<File> {
    use std::{os::windows::io::FromRawHandle, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SECURITY_ATTRIBUTES,
        },
        Storage::FileSystem::{CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL},
    };

    let path = wide_path(path);
    let mut descriptor = null_mut();
    // Apply a protected owner-only DACL at creation, before writing the key.
    // All pointers below reference live wide strings and Win32 allocations.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            windows_sys::core::w!("D:P(A;;GA;;;OW)"),
            1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let handle = CreateFileW(
            path.as_ptr(),
            GENERIC_WRITE,
            0,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        );
        let result = if handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(File::from_raw_handle(handle))
        };
        LocalFree(descriptor);
        result
    }
}
