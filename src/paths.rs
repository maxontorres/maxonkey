//! OS-standard vault location and owner-only permission handling (SR-030 – SR-032).

use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const VAULT_FILE: &str = "vault.sqlite3";

/// `%LOCALAPPDATA%\MaxonKey\data\vault.sqlite3`, `$XDG_DATA_HOME/maxonkey/vault.sqlite3`,
/// or `~/Library/Application Support/MaxonKey/vault.sqlite3`.
pub fn default_vault_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "MaxonKey")
        .map(|dirs| dirs.data_local_dir().join(VAULT_FILE))
}

/// Creates `dir` (and missing parents) readable only by the owner on Unix.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

/// Opens a directory handle and flushes it so a completed rename survives power loss (Unix).
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(dir)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// Human-readable warnings when the vault or its directory is accessible to others.
pub fn permission_warnings(vault: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let dir = vault.parent().filter(|p| !p.as_os_str().is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut check = |path: &Path, what: &str| {
            if let Ok(meta) = fs::metadata(path) {
                let mode = meta.permissions().mode() & 0o777;
                if mode & 0o077 != 0 {
                    warnings.push(format!(
                        "{what} is accessible to other users (mode {mode:03o}); run: chmod {} {}",
                        if meta.is_dir() { "700" } else { "600" },
                        path.display()
                    ));
                }
            }
        };
        if let Some(dir) = dir {
            check(dir, "vault directory");
        }
        check(vault, "vault file");
    }
    #[cfg(windows)]
    {
        let _ = dir;
        match windows_acl::broad_grants(vault) {
            Ok(groups) if !groups.is_empty() => warnings.push(format!(
                "vault file grants access to {}; restrict it to your account",
                groups.join(", ")
            )),
            Ok(_) => {}
            Err(error) => warnings.push(format!("could not read vault file permissions: {error}")),
        }
    }
    #[cfg(not(any(unix, windows)))]
    let _ = dir;
    warnings
}

#[cfg(windows)]
mod windows_acl {
    use std::{ffi::c_void, io, os::windows::ffi::OsStrExt, path::Path, ptr};

    use windows_sys::Win32::{
        Foundation::{ERROR_SUCCESS, LocalFree},
        Security::Authorization::{ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT},
        Security::{
            ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
            DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, PSECURITY_DESCRIPTOR,
        },
        System::SystemServices::ACCESS_ALLOWED_ACE_TYPE,
    };

    /// Well-known groups that would expose the vault to other local accounts.
    const BROAD: [(&str, &str); 3] = [
        ("S-1-1-0", "Everyone"),
        ("S-1-5-32-545", "Users"),
        ("S-1-5-11", "Authenticated Users"),
    ];

    /// Frees memory the security APIs allocated with `LocalAlloc`.
    struct LocalMemory(*mut c_void);

    impl Drop for LocalMemory {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the pointer came from a Windows API documented to require LocalFree.
                unsafe { LocalFree(self.0) };
            }
        }
    }

    pub fn broad_grants(path: &Path) -> io::Result<Vec<&'static str>> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated; out-pointers are valid for writes.
        let status = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let _descriptor = LocalMemory(descriptor);
        if dacl.is_null() {
            // A NULL DACL grants full access to everyone.
            return Ok(vec!["Everyone"]);
        }
        let mut info = ACL_SIZE_INFORMATION::default();
        // SAFETY: `dacl` points into `descriptor`, which outlives this call.
        let ok = unsafe {
            GetAclInformation(
                dacl,
                (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut found = Vec::new();
        for index in 0..info.AceCount {
            let mut ace: *mut c_void = ptr::null_mut();
            // SAFETY: `index` is below the ACE count reported for this ACL.
            if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: every ACE starts with an ACE_HEADER.
            let header = unsafe { &*(ace as *const ACE_HEADER) };
            if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
                continue;
            }
            let allowed = ace as *const ACCESS_ALLOWED_ACE;
            // SAFETY: for ACCESS_ALLOWED_ACE the SID begins at `SidStart`.
            let sid = unsafe { ptr::addr_of!((*allowed).SidStart) } as *mut c_void;
            let mut text: *mut u16 = ptr::null_mut();
            // SAFETY: `sid` is a valid SID inside the ACE; `text` receives a LocalAlloc string.
            if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let _text = LocalMemory(text.cast());
            // SAFETY: `text` is a NUL-terminated UTF-16 string.
            let sid_string = unsafe {
                let len = (0..).take_while(|&i| *text.add(i) != 0).count();
                String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
            };
            if let Some((_, name)) = BROAD.iter().find(|(id, _)| *id == sid_string)
                && !found.contains(name)
            {
                found.push(*name);
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_path_ends_with_vault_file() {
        if let Some(path) = default_vault_path() {
            assert!(path.ends_with(VAULT_FILE));
            assert!(path.is_absolute());
        }
    }

    /// ST-13: owner-only modes regardless of umask.
    #[cfg(target_os = "linux")]
    #[test]
    fn private_dir_is_owner_only_and_broad_modes_warn() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("a").join("b");
        // SAFETY: umask is process-wide; files created concurrently by other tests use
        // explicit modes, so a temporary umask of 0 cannot widen them.
        let previous = unsafe { libc::umask(0) };
        let result = create_private_dir(&dir);
        unsafe { libc::umask(previous) };
        result.unwrap();
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let vault = dir.join(VAULT_FILE);
        fs::write(&vault, b"x").unwrap();
        fs::set_permissions(&vault, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(permission_warnings(&vault).is_empty());
        fs::set_permissions(&vault, fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let warnings = permission_warnings(&vault);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("chmod 700"));
        assert!(warnings[1].contains("chmod 600"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_acl_check_reads_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join(VAULT_FILE);
        fs::write(&vault, b"x").unwrap();
        // The result depends on the runner's profile ACL; reading it must succeed.
        windows_acl::broad_grants(&vault).unwrap();
    }
}
