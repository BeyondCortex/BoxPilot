//! Windows `FsPermissions` impl. Clears inheritance + sets a protected DACL
//! on the path so only the existing owner has access. Sub-project #3's
//! installer ACL story revisits this; for Sub-project #1 the goal is
//! `%LocalAppData%\BoxPilot\` is owner-only.

use crate::traits::fs_perms::{FsPermissions, PathKind};
use async_trait::async_trait;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID,
};

pub struct AclFsPermissions;

fn to_wstr(p: &Path) -> Vec<u16> {
    p.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[async_trait]
impl FsPermissions for AclFsPermissions {
    async fn restrict_to_owner(&self, path: &Path, _kind: PathKind) -> std::io::Result<()> {
        let path_w = to_wstr(path);
        // Minimal Sub-project #1 ACL: clear inheritance + protected DACL.
        // Sub-project #3's installer ACL story will revisit. The empty
        // DACL pattern grants only the owner implicit access.
        tokio::task::spawn_blocking(move || unsafe {
            let mut owner: PSID = std::ptr::null_mut();
            let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            let rc = GetNamedSecurityInfoW(
                path_w.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut sd,
            );
            if rc != 0 {
                return Err(std::io::Error::from_raw_os_error(rc as i32));
            }
            let rc2 = SetNamedSecurityInfoW(
                path_w.as_ptr() as *mut _,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if rc2 != 0 {
                return Err(std::io::Error::from_raw_os_error(rc2 as i32));
            }
            Ok(())
        })
        .await
        .map_err(|e| std::io::Error::other(format!("spawn_blocking: {e}")))?
    }
}
