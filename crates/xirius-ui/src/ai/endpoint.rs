//! The application's private, machine-local MCP identity (MCP-007).
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use xirius_mcp::{
    Gateway, HttpConfig,
    background::{Background, serve_http},
    http::new_token,
};

pub(super) struct Started {
    pub server: Background,
    pub busy_port: Option<u16>,
}

#[cfg(not(test))]
pub(super) fn path() -> io::Result<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base =
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state")));
    Ok(base
        .ok_or_else(|| io::Error::other("MCP の保存先が見つかりません"))?
        .join("Xirius/mcp-endpoint.json"))
}

fn private_file(path: &Path, new: bool) -> io::Result<File> {
    // design-lint: allow(ui-blocking-io) MCP-007: endpoint storage runs in the MCP startup worker; synchronous callers are tests only.
    let mut opts = OpenOptions::new();
    // design-lint: allow(ui-blocking-io) MCP-007: endpoint storage runs in the MCP startup worker; synchronous callers are tests only.
    opts.read(true).write(true);
    if new {
        opts.create_new(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // WRITE_DAC permits replacing the inherited ACL before writing any secret.
        opts.access_mode(0x80000000 | 0x40000000 | 0x00040000);
    }
    let file = opts.open(path)?;
    protect(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn protect(file: &File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // design-lint: allow(ui-blocking-io) MCP-007: endpoint storage runs in the MCP startup worker; synchronous callers are tests only.
    file.set_permissions(fs::Permissions::from_mode(0o600))
}

#[cfg(windows)]
fn protect(file: &File) -> io::Result<()> {
    use std::{os::windows::io::AsRawHandle, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{Authorization::*, *},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    // All buffers and handles live through their native calls. The ACL contains only
    // this process user's SID and disables inheritance, including on existing files.
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut bytes = 0;
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut bytes);
        let mut buf = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
        let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), bytes, &mut bytes);
        let error = io::Error::last_os_error();
        CloseHandle(token);
        if ok == 0 {
            return Err(error);
        }
        let sid = (*(buf.as_ptr().cast::<TOKEN_USER>())).User.Sid;
        let mut text = ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let sid_text = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        let sddl: Vec<u16> = format!("D:P(A;;FA;;;{sid_text})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let ok = SetKernelObjectSecurity(
            file.as_raw_handle(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        );
        let error = io::Error::last_os_error();
        LocalFree(descriptor);
        if ok == 0 {
            return Err(error);
        }
    }
    Ok(())
}

fn read(path: &Path) -> io::Result<Option<(u16, String)>> {
    let file = match private_file(path, false) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let value: serde_json::Value = match serde_json::from_reader(file) {
        Ok(v) => v,
        Err(e) if e.is_io() => return Err(io::Error::other("MCP の接続情報を読み込めません")),
        Err(_) => return Ok(None),
    };
    Ok(value["port"]
        .as_u64()
        .filter(|p| (20000..32000).contains(p))
        .zip(
            value["token"]
                .as_str()
                .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())),
        )
        .map(|(p, t)| (p as u16, t.to_owned())))
}

fn save(path: &Path, server: &Background) -> io::Result<()> {
    let temp = path.with_extension(format!("{}.tmp", new_token()?));
    let result = (|| {
        let mut file = private_file(&temp, true)?;
        let value = serde_json::json!({"port":server.addr.port(),"token":server.token});
        // design-lint: allow(ui-blocking-io) MCP-007: endpoint storage runs in the MCP startup worker; synchronous callers are tests only.
        file.write_all(serde_json::to_string(&value)?.as_bytes())?;
        // design-lint: allow(ui-blocking-io) MCP-007: endpoint storage runs in the MCP startup worker; synchronous callers are tests only.
        file.sync_all()?;
        drop(file);
        // design-lint: allow(ui-blocking-io) MCP-007: endpoint storage runs in the MCP startup worker; synchronous callers are tests only.
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        // design-lint: allow(ui-blocking-io) MCP-007: endpoint storage runs in the MCP startup worker; synchronous callers are tests only.
        let _ = fs::remove_file(temp);
    }
    result
}

pub(super) fn start(gateway: Gateway, path: PathBuf, renew: bool) -> io::Result<Started> {
    // design-lint: allow(ui-blocking-io) MCP-007: called by the MCP startup worker, or synchronous storage tests.
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| io::Error::other("MCP の保存先がありません"))?,
    )?;
    let lock_path = path.with_extension("lock");
    let lock = match private_file(&lock_path, true) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => private_file(&lock_path, false)?,
        Err(e) => return Err(e),
    };
    // design-lint: allow(ui-blocking-io) MCP-007: endpoint storage runs in the MCP startup worker; synchronous callers are tests only.
    lock.lock()?;
    let saved = read(&path)?;
    if let Some((port, token)) = &saved
        && !renew
    {
        let cfg = HttpConfig {
            port: *port,
            token: Some(token.clone()),
            ..Default::default()
        };
        return match serve_http(gateway.clone(), cfg.clone()) {
            Ok(server) => Ok(Started {
                server,
                busy_port: None,
            }),
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => Ok(Started {
                server: serve_http(gateway, HttpConfig { port: 0, ..cfg })?,
                busy_port: Some(*port),
            }),
            Err(e) => Err(e),
        };
    }
    let mut token = new_token()?;
    while saved.as_ref().is_some_and(|(_, old)| old == &token) {
        token = new_token()?;
    }
    let offset = u16::from_str_radix(&new_token()?[..4], 16).unwrap_or(0) % 12000;
    for step in 0..12000u16 {
        let port = 20000 + (offset + step) % 12000;
        if saved.as_ref().is_some_and(|(old, _)| *old == port) {
            continue;
        }
        match serve_http(
            gateway.clone(),
            HttpConfig {
                port,
                token: Some(token.clone()),
                ..Default::default()
            },
        ) {
            Ok(server) => {
                save(&path, &server)?;
                return Ok(Started {
                    server,
                    busy_port: None,
                });
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
                ) =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "MCP の空きポートが見つかりません",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, net::TcpStream, time::Duration};

    fn launch(path: &Path, renew: bool) -> Started {
        let (host, _jobs) = xirius_mcp::bridge(xirius_session::commands::shared_registry());
        start(
            Gateway::hosted(host, "AI", xirius_mcp::Scope::Edit),
            path.to_owned(),
            renew,
        )
        .unwrap()
    }

    fn request(server: &Background, token: &str) -> String {
        let mut stream = TcpStream::connect_timeout(&server.addr, Duration::from_secs(5)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;
        write!(stream, "POST /mcp HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}", server.addr, token, body.len(), body).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn saved_identity_survives_restart_and_admin_identity_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("endpoint.json");
        let first = launch(&path, false).server;
        let (url, token, admin) = (
            first.mcp_url(),
            first.token.clone(),
            first.admin_token.clone(),
        );
        assert_ne!(token, admin);
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2);
        assert!(!value.to_string().contains(&admin));
        first.stop();
        let second = launch(&path, false);
        assert_eq!(second.server.mcp_url(), url);
        assert_eq!(second.server.token, token);
        assert_ne!(second.server.admin_token, admin);
        assert_eq!(second.busy_port, None);
        second.server.stop();
    }

    #[test]
    fn busy_port_is_temporary_and_renewal_replaces_both_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("endpoint.json");
        let first = launch(&path, false).server;
        let saved = fs::read(&path).unwrap();
        let fallback = launch(&path, false);
        assert_eq!(fallback.busy_port, Some(first.addr.port()));
        assert_ne!(fallback.server.addr.port(), first.addr.port());
        assert_eq!(fallback.server.token, first.token);
        assert_eq!(fs::read(&path).unwrap(), saved);
        fallback.server.stop();
        let renewed = launch(&path, true).server;
        assert_ne!(renewed.addr.port(), first.addr.port());
        assert_ne!(renewed.token, first.token);
        assert!(request(&renewed, &first.token).starts_with("HTTP/1.1 401"));
        let answer = request(&renewed, &renewed.token);
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        assert!(!answer.contains("起動のたびに変わります"));
        let expected = (renewed.addr.port(), renewed.token.clone());
        first.stop();
        renewed.stop();
        assert_eq!(read(&path).unwrap(), Some(expected));
    }

    #[test]
    fn empty_and_corrupt_files_are_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("endpoint.json");
        for bad in ["", "{", "{\"port\":80,\"token\":\"short\"}"] {
            fs::write(&path, bad).unwrap();
            let server = launch(&path, false).server;
            assert!((20000..32000).contains(&server.addr.port()));
            assert_eq!(
                read(&path).unwrap(),
                Some((server.addr.port(), server.token.clone()))
            );
            server.stop();
        }
    }

    #[cfg(unix)]
    #[test]
    fn file_is_owner_only_even_after_permissions_were_widened() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("endpoint.json");
        launch(&path, false).server.stop();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        read(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(windows)]
    #[test]
    fn file_acl_is_protected_and_grants_only_its_owner() {
        use std::{os::windows::io::AsRawHandle, ptr};
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{Authorization::*, *},
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("endpoint.json");
        launch(&path, false).server.stop();
        let file = private_file(&path, false).unwrap();
        unsafe {
            let mut owner = ptr::null_mut();
            let mut acl = ptr::null_mut();
            let mut sd = ptr::null_mut();
            assert_eq!(
                GetSecurityInfo(
                    file.as_raw_handle(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                    &mut owner,
                    ptr::null_mut(),
                    &mut acl,
                    ptr::null_mut(),
                    &mut sd
                ),
                0
            );
            let mut control = 0;
            let mut revision = 0;
            assert_ne!(
                GetSecurityDescriptorControl(sd, &mut control, &mut revision),
                0
            );
            assert_ne!(control & SE_DACL_PROTECTED, 0);
            assert!(!acl.is_null());
            assert_eq!((*acl).AceCount, 1);
            let mut ace = ptr::null_mut();
            assert_ne!(GetAce(acl, 0, &mut ace), 0);
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            assert_eq!(ace.Header.AceType, 0);
            assert_ne!(
                EqualSid(owner, ptr::addr_of!(ace.SidStart).cast_mut().cast()),
                0
            );
            LocalFree(sd);
        }
    }
}
